//! The core Entity Component System of the engine.

// rustc lints
#![warn(
    let_underscore,
    nonstandard_style,
    unused,
    explicit_outlives_requirements,
    meta_variable_misuse,
    missing_debug_implementations,
    missing_docs,
    non_ascii_idents,
    noop_method_call,
    pointer_structural_match,
    trivial_casts,
    trivial_numeric_casts
)]
// clippy lints
#![warn(
    clippy::cognitive_complexity,
    clippy::dbg_macro,
    clippy::if_then_some_else_none,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::rc_mutex,
    clippy::unwrap_used,
    clippy::large_enum_variant
)]

pub mod filter;
pub mod logging;
pub mod profiling;
pub mod systems;

use crate::systems::SystemError::CannotRunSequentially;
use crate::systems::{
    ComponentIndex, IntoSystem, System, SystemError, SystemParameters, SystemResult,
};
use crate::BasicApplicationError::ScheduleGeneration;
use core::panic;
use crossbeam::channel::{bounded, Receiver, Sender, TryRecvError};
use fnv::{FnvHashMap, FnvHashSet};
use nohash_hasher::IsEnabled;
use nohash_hasher::NoHashHasher;
use std::any;
use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::Debug;
use std::hash::BuildHasherDefault;
use std::hash::Hash;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};
use thiserror::Error;

/// Builds and configures an [`Application`] instance.
pub trait ApplicationBuilder: Default {
    /// Which type of application is constructed.
    type App;

    /// Registers a new system to run in the world.
    fn add_system<System, Parameters>(self, system: System) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters;

    /// Registers multiple new systems to run in the world.
    fn add_systems<System, Parameters>(self, systems: impl IntoIterator<Item = System>) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters;

    /// Completes the building and returns the created [`Application`].
    fn build(self) -> Self::App;
}

/// Constructs a [`BasicApplication`].
#[derive(Debug, Default)]
pub struct BasicApplicationBuilder {
    systems: Vec<Box<dyn System>>,
}

impl ApplicationBuilder for BasicApplicationBuilder {
    type App = BasicApplication;

    fn add_system<System, Parameters>(mut self, system: System) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.systems.push(Box::new(system.into_system()));
        self
    }

    fn add_systems<System, Parameters>(mut self, systems: impl IntoIterator<Item = System>) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        for system in systems {
            self = self.add_system(system);
        }
        self
    }

    fn build(self) -> Self::App {
        BasicApplication {
            world: Default::default(),
            systems: self.systems,
        }
    }
}

/// An error in the application.
#[derive(Error, Debug)]
pub enum BasicApplicationError {
    /// Failed to generate schedule for given systems.
    #[error("failed to generate schedule for given systems")]
    ScheduleGeneration(#[source] ScheduleError),
    /// Failed to execute systems.
    #[error("failed to execute systems")]
    Execution(#[source] ExecutionError),
    /// Failed to execute world operation.
    #[error("failed to perform world operation")]
    World(#[source] WorldError),
    /// Failed to add component to entity.
    #[error("failed to add component {0:?} to entity {1:?}")]
    ComponentAdding(#[source] WorldError, String, Entity),
}

/// Whether an operation on the application succeeded.
pub type BasicAppResult<T, E = BasicApplicationError> = Result<T, E>;

/// A basic type of [`Application`], with not much extra functionality.
#[derive(Default, Debug)]
pub struct BasicApplication {
    /// The place in which all components and entities roam freely, living their best lives.
    pub world: World,
    /// All [`System`]s that will be executed in the application's [`World`].
    pub systems: Vec<Box<dyn System>>,
}

/// The entry-point of the entire program, containing all of the entities, components and systems.
pub trait Application {
    /// The type of errors returned by application methods.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Spawns a new entity in the world.
    fn create_entity(&mut self) -> Result<Entity, Self::Error>;

    /// Adds a new component to a given entity.
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> Result<(), Self::Error>;

    /// Removes a component type from a given entity.
    fn remove_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
    ) -> Result<(), Self::Error>;

    /// Adds a new [`System`] to the application, after construction has already finished.
    fn add_system<System, Parameters>(&mut self, system: System)
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters;

    /// Starts the application. This function does not return until the shutdown command has
    /// been received.
    fn run<'systems, E: Executor<'systems>, S: Schedule<'systems>>(
        &'systems mut self,
        shutdown_receiver: Receiver<()>,
    ) -> Result<(), Self::Error>;
}

impl Application for BasicApplication {
    type Error = BasicApplicationError;

    fn create_entity(&mut self) -> Result<Entity, Self::Error> {
        self.world
            .create_new_entity()
            .map_err(BasicApplicationError::World)
    }

    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> Result<(), Self::Error> {
        let component_text = format!("{component:?}");
        self.world
            .add_component_to_entity(entity, component)
            .map_err(|error| BasicApplicationError::ComponentAdding(error, component_text, entity))
    }

    fn remove_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
    ) -> Result<(), Self::Error> {
        self.world
            .remove_component_type_from_entity::<ComponentType>(entity)
            .map_err(BasicApplicationError::World)
    }

    fn add_system<System, Parameters>(&mut self, system: System)
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.systems.push(Box::new(system.into_system()));
    }

    fn run<'systems, E: Executor<'systems>, S: Schedule<'systems>>(
        &'systems mut self,
        shutdown_receiver: Receiver<()>,
    ) -> Result<(), Self::Error> {
        let schedule = S::generate(&self.systems).map_err(ScheduleGeneration)?;
        let mut executor = E::default();
        executor
            .execute(schedule, &self.world, shutdown_receiver)
            .map_err(BasicApplicationError::Execution)
    }
}

/// A way of executing a `ecs::Schedule`.
pub trait Executor<'systems>: Default {
    /// Executes systems in a world according to a given schedule,
    /// until the `Sender<()>` corresponding to the given `shutdown_receiver` is dropped.
    fn execute<S: Schedule<'systems>>(
        &mut self,
        schedule: S,
        world: &'systems World,
        shutdown_receiver: Receiver<()>,
    ) -> ExecutionResult<()>;

    /// Executes systems in a world according to a given schedule for a single tick,
    /// meaning all systems get to execute once.
    fn execute_once<S: Schedule<'systems>>(
        &mut self,
        schedule: &mut S,
        world: &'systems World,
    ) -> ExecutionResult<()>;
}

/// An error occurred during execution.
#[derive(Error, Debug)]
pub enum ExecutionError {
    /// Could not execute due to error in schedule.
    #[error("could not execute due to error in schedule")]
    Schedule(#[source] ScheduleError),
    /// Could not execute system.
    #[error("could not execute system")]
    System(#[source] SystemError),
    /// The executor has not been properly initialized.
    #[error("the executor has not been properly initialized")]
    Uninitialized,
}

/// Whether an execution succeeded.
pub type ExecutionResult<T, E = ExecutionError> = Result<T, E>;

/// Runs systems in sequence, one after the other.
#[derive(Default, Debug)]
pub struct Sequential;

impl<'systems> Executor<'systems> for Sequential {
    fn execute<S: Schedule<'systems>>(
        &mut self,
        mut schedule: S,
        world: &World,
        shutdown_receiver: Receiver<()>,
    ) -> ExecutionResult<()> {
        while let Err(TryRecvError::Empty) = shutdown_receiver.try_recv() {
            for system in schedule
                .currently_executable_systems()
                .map_err(ExecutionError::Schedule)?
            {
                system.run(world).map_err(ExecutionError::System)?;
            }
        }
        Ok(())
    }

    fn execute_once<S: Schedule<'systems>>(
        &mut self,
        schedule: &mut S,
        world: &'systems World,
    ) -> ExecutionResult<()> {
        loop {
            let systems =
                schedule.currently_executable_systems_with_reaction(NewTickReaction::ReturnError);
            match systems {
                Ok(systems) => {
                    for system in systems {
                        system.run(world).map_err(ExecutionError::System)?;
                    }
                }
                Err(ScheduleError::NewTick) => {
                    return Ok(());
                }
                Err(schedule_error) => {
                    return Err(ExecutionError::Schedule(schedule_error));
                }
            }
        }
    }
}

/// An error occurred during a schedule operation.
#[derive(Error, Debug)]
pub enum ScheduleError {
    /// Failed to generate schedule from the given systems.
    #[error("failed to generate schedule from the given systems: {0:?}")]
    Generation(String, #[source] Box<dyn Error + Send + Sync>),
    /// Could not get next systems in schedule to execute.
    #[error("could not get next systems in schedule to execute")]
    NextSystems(#[source] Box<dyn Error + Send + Sync>),
    /// A new tick will begin next time systems are requested.
    #[error("a new tick will begin next time systems are requested")]
    NewTick,
}

/// Whether a schedule operation succeeded.
pub type ScheduleResult<T, E = ScheduleError> = Result<T, E>;

/// How the schedule should handle starting a new tick.
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
pub enum NewTickReaction {
    /// When a new tick is going to begin, don't immediately start it.
    /// Instead, return an error indicating that the next call will begin a new tick.
    ReturnError,
    /// Begin the new tick immediately, returning the start-systems of the new tick.
    ReturnNewTick,
}

/// An ordering of `ecs::System` executions.
pub trait Schedule<'systems>: Debug + Sized + Send + Sync {
    /// Creates a scheduling of the given systems.
    fn generate(systems: &'systems [Box<dyn System>]) -> ScheduleResult<Self>;

    /// Gets systems that are safe to execute concurrently right now.
    /// If none are available, this function __blocks__ until some are.
    ///
    /// The returned value is a [`SystemExecutionGuard`] which keeps track of when the
    /// system is executed, so this function can stop blocking when dependencies are cleared.
    ///
    /// Calls to this function are not idempotent, meaning after systems have been returned
    /// once they will not be returned again until the next tick (when all systems have run once).
    ///
    /// When a new tick begins, this function will immediately return the systems of the
    /// new tick. If you wish to be given a warning when a new tick begins, take a look at
    /// [Schedule::currently_executable_systems_with_reaction].
    fn currently_executable_systems(
        &mut self,
    ) -> ScheduleResult<Vec<SystemExecutionGuard<'systems>>> {
        self.currently_executable_systems_with_reaction(NewTickReaction::ReturnNewTick)
    }

    /// The same as [Schedule::currently_executable_systems], but you can control how
    /// new ticks will be handled using [NewTickReaction].
    fn currently_executable_systems_with_reaction(
        &mut self,
        new_tick_reaction: NewTickReaction,
    ) -> ScheduleResult<Vec<SystemExecutionGuard<'systems>>>;
}

/// A wrapper around a system that monitors when the system has been executed.
#[derive(Debug)]
pub struct SystemExecutionGuard<'system> {
    /// A system to be executed.
    pub system: &'system dyn System,
    /// When this sender is dropped, that signals to the [`Schedule`] that this system
    /// is finished executing.
    pub finished_sender: Sender<()>,
}

impl<'system> SystemExecutionGuard<'system> {
    /// Creates a new execution guard. The returned tuple contains a receiver that will be
    /// notified when the system has executed.
    pub fn create(system: &'system dyn System) -> (Self, Receiver<()>) {
        let (finished_sender, finished_receiver) = bounded(1);
        let guard = Self {
            system,
            finished_sender,
        };
        (guard, finished_receiver)
    }

    /// Execute the system.
    pub fn run(&self, world: &World) -> SystemResult<()> {
        let system = self
            .system
            .try_as_sequentially_iterable()
            .ok_or(CannotRunSequentially)?;
        system.run(world)
    }
}

/// Schedules systems in no particular order, with no regard to dependencies.
#[derive(Default, Debug)]
pub struct Unordered<'systems> {
    systems: &'systems [Box<dyn System>],
    wait_until_next_call: bool,
}

impl<'systems> Schedule<'systems> for Unordered<'systems> {
    fn generate(systems: &'systems [Box<dyn System>]) -> ScheduleResult<Self> {
        Ok(Self {
            systems,
            wait_until_next_call: false,
        })
    }

    fn currently_executable_systems_with_reaction(
        &mut self,
        new_tick_reaction: NewTickReaction,
    ) -> ScheduleResult<Vec<SystemExecutionGuard<'systems>>> {
        let all_systems = self
            .systems
            .iter()
            .map(|system| system.as_ref())
            .map(|system| SystemExecutionGuard::create(system).0)
            .collect();

        if new_tick_reaction == NewTickReaction::ReturnError && self.wait_until_next_call {
            // Make sure next call to this function will return systems.
            self.wait_until_next_call = false;
            Err(ScheduleError::NewTick)
        } else if new_tick_reaction == NewTickReaction::ReturnError && !self.wait_until_next_call {
            // Make sure next call to this function will not return systems.
            self.wait_until_next_call = true;

            Ok(all_systems)
        } else {
            Ok(all_systems)
        }
    }
}

/// An error occurred during a archetype operation.
#[derive(Error, Debug)]
pub enum ArchetypeError {
    /// Could not find component index of entity
    #[error("could not find component index of entity: {0:?}")]
    MissingEntityIndex(Entity),

    /// Could not borrow component vec
    #[error("could not borrow component vec of type: {0:?}")]
    CouldNotBorrowComponentVec(TypeId),

    /// Component vec not found for type id
    #[error("component vec not found for type id: {0:?}")]
    MissingComponentType(TypeId),

    /// Archetype already contains entity
    #[error("archetype already contains entity: {0:?}")]
    EntityAlreadyExists(Entity),
}

/// Whether a archetype operation succeeded.
pub type ArchetypeResult<T, E = ArchetypeError> = Result<T, E>;

/// Stores components associated with entity ids.
#[derive(Debug, Default)]
struct Archetype {
    component_typeid_to_component_vec: FnvHashMap<TypeId, Box<dyn ComponentVec>>,
    entity_to_component_index:
        HashMap<Entity, ComponentIndex, BuildHasherDefault<NoHashHasher<Entity>>>,
    entity_order: Vec<Entity>,
}

/// Newly created entities with no components on them, are placed in this archetype.
const EMPTY_ENTITY_ARCHETYPE_INDEX: ArchetypeIndex = 0;

impl Archetype {
    /// Adds an `entity_id` to keep track of and store components for.
    ///
    /// Returns error if entity with `id` has been stored previously.
    fn store_entity(&mut self, entity: Entity) -> ArchetypeResult<()> {
        if !self.entity_to_component_index.contains_key(&entity) {
            let entity_index = self.entity_to_component_index.len();

            self.entity_to_component_index.insert(entity, entity_index);

            self.entity_order.push(entity);

            Ok(())
        } else {
            Err(ArchetypeError::EntityAlreadyExists(entity))
        }
    }

    /// Returns a `ReadComponentVec` with the specified generic type `ComponentType` if it is stored.
    fn borrow_component_vec<ComponentType: Debug + Send + Sync + 'static>(
        &self,
    ) -> ReadComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        self.component_typeid_to_component_vec
            .get(&component_typeid)
            .map(Box::as_ref)
            .and_then(borrow_component_vec)
    }

    /// Returns a `WriteComponentVec` with the specified generic type `ComponentType` if it is stored.
    fn borrow_component_vec_mut<ComponentType: Debug + Send + Sync + 'static>(
        &self,
    ) -> WriteComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        self.component_typeid_to_component_vec
            .get(&component_typeid)
            .map(Box::as_ref)
            .and_then(borrow_component_vec_mut)
    }

    /// Adds a component of type `ComponentType` to archetype.
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        component: ComponentType,
    ) -> ArchetypeResult<()> {
        let mut component_vec = self.borrow_component_vec_mut::<ComponentType>().ok_or(
            ArchetypeError::CouldNotBorrowComponentVec(TypeId::of::<ComponentType>()),
        )?;
        component_vec.push(component);

        Ok(())
    }

    /// Adds a component vec of type `ComponentType` if no such vec already exists.
    ///
    /// This function is idempotent when trying to add the same `ComponentType` multiple times.
    fn add_component_vec<ComponentType: Debug + Send + Sync + 'static>(&mut self) {
        if !self.contains_component_type::<ComponentType>() {
            let raw_component_vec = create_raw_component_vec::<ComponentType>();

            let component_typeid = TypeId::of::<ComponentType>();
            self.component_typeid_to_component_vec
                .insert(component_typeid, raw_component_vec);
        }
    }

    /// Returns `true` if the archetype stores components of type ComponentType.
    fn contains_component_type<ComponentType: Debug + Send + Sync + 'static>(&self) -> bool {
        self.component_typeid_to_component_vec
            .contains_key(&TypeId::of::<ComponentType>())
    }

    // todo(#101): make private
    fn update_source_archetype_after_entity_move(&mut self, entity: Entity) {
        let source_component_vec_idx = *self.entity_to_component_index.get(&entity).expect(
            "Entity should yield a component index
             in archetype since the the relevant archetype
              was fetched from the entity itself.",
        );
        // Update old archetypes component vec index after moving entity
        if let Some(&swapped_entity) = self.entity_order.last() {
            self.entity_to_component_index
                .insert(swapped_entity, source_component_vec_idx);
        }
        // Remove entity component index existing in its old archetype
        self.entity_order.swap_remove(source_component_vec_idx);

        self.entity_to_component_index.remove(&entity);
    }
    fn component_types(&self) -> FnvHashSet<TypeId> {
        self.component_typeid_to_component_vec
            .keys()
            .cloned()
            .collect()
    }
}

/// An error occurred during a world operation.
#[derive(Error, Debug)]
pub enum WorldError {
    /// Could not find archetype with the given index.
    #[error("could not find archetype with the given index: {0:?}")]
    ArchetypeDoesNotExist(ArchetypeIndex),
    /// Could not add component to archetype with the given index.
    #[error("could not add component to archetype with the given index: {0:?}")]
    CouldNotAddComponent(#[source] ArchetypeError),
    /// Could not move component to archetype.
    #[error("could not move component to archetype")]
    CouldNotMoveComponent(#[source] ArchetypeError),
    /// Could not find the given component type.
    #[error("could not find the given component type: {0:?}")]
    ComponentTypeDoesNotExist(TypeId),
    /// Could not find the given entity.
    #[error("could not find the given entity: {0:?}")]
    EntityDoesNotExist(Entity),
    /// Component of same type already exists for entity
    #[error("component of same type {1:?} already exists for entity {0:?}")]
    ComponentTypeAlreadyExistsForEntity(Entity, TypeId),
    /// Component type does not exist for entity.
    #[error("component of type {1:?} does not exist for entity {0:?}")]
    ComponentTypeNotPresentForEntity(Entity, TypeId),
}

/// Whether a world operation succeeded.
pub type WorldResult<T, E = WorldError> = Result<T, E>;

/// The index of an archtype in a vec.
type ArchetypeIndex = usize;

/// Represents the simulated world.
#[derive(Debug)]
pub struct World {
    entities: Vec<Entity>,
    /// Relates a unique `Entity Id` to the `Archetype` that stores it.
    /// The HashMap returns the corresponding `index` of the `Archetype` stored in the `World.archetypes` vector.
    entity_to_archetype_index:
        HashMap<Entity, ArchetypeIndex, BuildHasherDefault<NoHashHasher<Entity>>>,
    /// Stores all `Archetypes`. The `index` of each `Archetype` cannot be
    /// changed without also updating the `entity_id_to_archetype_index` HashMap,
    /// since it needs to point to the correct `Archetype`.
    archetypes: Vec<Archetype>,
    /// `component_typeid_to_archetype_indices` is a HashMap relating all Component `TypeId`s to the `Archetype`s that store them.
    /// Its purpose it to allow querying of `Archetypes` that contain a specific set of `Components`.
    ///
    /// For example: If you want to query all Archetypes that contain components (A,B)
    /// you will first call `get(TypeId::of:<A>())` and then `get(TypeId::of:<B>())`.
    /// This will result in two vectors containing the indices of the `Archetype`s that store these types.
    /// By taking the intersection of these vectors you will know which `Archetype` contain both A and B.
    /// This could be the archetypes: (A,B), (A,B,C), (A,B,...) etc.
    component_typeid_to_archetype_indices:
        FnvHashMap<TypeId, HashSet<ArchetypeIndex, BuildHasherDefault<NoHashHasher<usize>>>>,
    component_typeids_set_to_archetype_index: FnvHashMap<Vec<TypeId>, ArchetypeIndex>,
}

type ReadComponentVec<'a, ComponentType> = Option<RwLockReadGuard<'a, Vec<ComponentType>>>;
type WriteComponentVec<'a, ComponentType> = Option<RwLockWriteGuard<'a, Vec<ComponentType>>>;
type TargetArchSourceTargetIDs = (
    Option<ArchetypeIndex>,
    FnvHashSet<TypeId>,
    FnvHashSet<TypeId>,
);
enum ArchetypeMutation {
    Removal(Entity),
    Addition(Entity),
}

impl World {
    /// Returns the archetype index of the archetype that contains all
    /// components types that the entity is tied to +- the generic type
    /// supplied to the function given Addition/ Removal enum status.
    /// `None` is returned if no archetype containing only the sought after component types exist.
    /// The type ids contained within the archetype the entity is existing in and the type ids for
    /// sought archetype are also returned.
    ///
    /// Set mutation to ArchetypeMutation::Addition(entity)/ ArchetypeMutation::Removal(entity)
    /// depending on if the supplied generic type should be added to or
    /// removed from the specified entity.
    ///
    /// For example call world.find_target_archetype::<u32>(ArchetypeMutation::Removal(entity))
    /// to fetch the archetype index of the archetype
    /// containing all component types except u32 that the entity is tied to.
    ///
    /// world.find_target_archetype::<u32>(ArchetypeMutation::Addition(entity))
    /// would return the archetype index of the
    /// archetype containing all component types that the entity is tied to with the addition
    /// of u32.
    fn find_target_archetype<ComponentType: Debug + Send + Sync + 'static>(
        &self,
        mutation: ArchetypeMutation,
    ) -> WorldResult<TargetArchSourceTargetIDs> {
        let source_archetype_type_ids: FnvHashSet<TypeId>;

        let mut target_archetype_type_ids: FnvHashSet<TypeId>;

        match mutation {
            ArchetypeMutation::Removal(nested_entity) => {
                let source_archetype = self.get_archetype_of_entity(nested_entity)?;

                source_archetype_type_ids = source_archetype.component_types();

                target_archetype_type_ids = source_archetype_type_ids.clone();

                if !target_archetype_type_ids.remove(&TypeId::of::<ComponentType>()) {
                    return Err(WorldError::ComponentTypeNotPresentForEntity(
                        nested_entity,
                        TypeId::of::<ComponentType>(),
                    ));
                }
            }

            ArchetypeMutation::Addition(nested_entity) => {
                let source_archetype = self.get_archetype_of_entity(nested_entity)?;

                source_archetype_type_ids = source_archetype.component_types();

                target_archetype_type_ids = source_archetype_type_ids.clone();

                target_archetype_type_ids.insert(TypeId::of::<ComponentType>());

                if source_archetype_type_ids.contains(&TypeId::of::<ComponentType>()) {
                    return Err(WorldError::ComponentTypeAlreadyExistsForEntity(
                        nested_entity,
                        TypeId::of::<ComponentType>(),
                    ));
                }
            }
        }

        let target_archetype_type_ids_vec: Vec<TypeId> = target_archetype_type_ids
            .clone()
            .into_iter()
            .collect::<Vec<TypeId>>();

        let maybe_target_archetype =
            self.get_exactly_matching_archetype(&target_archetype_type_ids_vec);

        Ok((
            maybe_target_archetype,
            source_archetype_type_ids,
            target_archetype_type_ids,
        ))
    }

    fn move_entity_components_between_archetypes(
        &mut self,
        entity: Entity,
        target_archetype_idx: ArchetypeIndex,
        components_to_move: FnvHashSet<TypeId>,
    ) -> WorldResult<()> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let (source_archetype, target_archetype) = get_mut_at_two_indices(
            source_archetype_index,
            target_archetype_idx,
            &mut self.archetypes,
        );

        let source_component_idx = *source_archetype
            .entity_to_component_index
            .get(&entity)
            .expect("Entity should have a component index tied to it since it is already established to be existing within the archetype");

        for type_id in components_to_move {
            let source_component_vec = source_archetype
                .component_typeid_to_component_vec
                .get(&type_id)
                .expect(
                    "Type that tried to be fetched should exist
                     in archetype since types are fetched from this archetype originally.",
                );

            source_component_vec
                .move_element(source_component_idx, target_archetype)
                .map_err(WorldError::CouldNotMoveComponent)?;
        }

        target_archetype
            .store_entity(entity)
            .map_err(WorldError::CouldNotMoveComponent)?;

        self.entity_to_archetype_index
            .insert(entity, target_archetype_idx);

        Ok(())
    }

    fn move_entity_components_to_new_archetype(
        &mut self,
        entity: Entity,
        components_to_move: FnvHashSet<TypeId>,
    ) -> WorldResult<ArchetypeIndex> {
        let new_archetype = Archetype::default();
        let new_archetype_index: ArchetypeIndex = self.archetypes.len();
        self.archetypes.push(new_archetype);

        for component_type in &components_to_move {
            self.component_typeid_to_archetype_indices
                .get_mut(component_type)
                .expect("Type ID should exist for previously existing types")
                .insert(new_archetype_index);
        }

        self.move_entity_components_between_archetypes(
            entity,
            new_archetype_index,
            components_to_move,
        )?;

        Ok(new_archetype_index)
    }

    fn add_component_to_entity<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> WorldResult<()> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let add = ArchetypeMutation::Addition(entity);

        let (target_archetype_exists, source_type_ids, target_type_ids) =
            self.find_target_archetype::<ComponentType>(add)?;

        match target_archetype_exists {
            Some(target_archetype_idx) => {
                self.move_entity_components_between_archetypes(
                    entity,
                    target_archetype_idx,
                    source_type_ids,
                )?;

                let target_archetype = self.archetypes
                    .get_mut(target_archetype_idx)
                    .expect("Archetype should exist since it was the requirement for entering this matching case");
                target_archetype
                    .add_component(component)
                    .map_err(WorldError::CouldNotAddComponent)?;
            }
            None => {
                let target_archetype_idx =
                    self.move_entity_components_to_new_archetype(entity, source_type_ids)?;

                let mut target_type_ids_vec: Vec<TypeId> = target_type_ids.into_iter().collect();
                target_type_ids_vec.sort();
                self.component_typeids_set_to_archetype_index
                    .insert(target_type_ids_vec, target_archetype_idx);

                let target_archetype = self
                    .archetypes
                    .get_mut(target_archetype_idx)
                    .expect("This should be added in the previous function call");

                // Handle incoming component
                target_archetype.add_component_vec::<ComponentType>();
                let archetype_indices = self
                    .component_typeid_to_archetype_indices
                    .entry(TypeId::of::<ComponentType>())
                    .or_default();
                archetype_indices.insert(target_archetype_idx);

                target_archetype
                    .add_component(component)
                    .map_err(WorldError::CouldNotAddComponent)?;
            }
        }

        let source_archetype = self
            .archetypes
            .get_mut(source_archetype_index)
            .expect("Source archetype has already been fetched previously");

        source_archetype.update_source_archetype_after_entity_move(entity);

        Ok(())
    }

    fn remove_component_type_from_entity<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
    ) -> WorldResult<()> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let removal = ArchetypeMutation::Removal(entity);
        let (target_archetype_exists, _, target_type_ids) =
            self.find_target_archetype::<ComponentType>(removal)?;

        match target_archetype_exists {
            Some(target_archetype_idx) => {
                self.move_entity_components_between_archetypes(
                    entity,
                    target_archetype_idx,
                    target_type_ids,
                )?;
            }
            None => {
                let mut target_type_ids_vec: Vec<TypeId> =
                    target_type_ids.clone().into_iter().collect();
                let target_archetype_idx =
                    self.move_entity_components_to_new_archetype(entity, target_type_ids)?;

                target_type_ids_vec.sort();
                self.component_typeids_set_to_archetype_index
                    .insert(target_type_ids_vec, target_archetype_idx);
            }
        }

        let source_archetype = self
            .archetypes
            .get_mut(source_archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_index))?;

        let source_component_vec = source_archetype
            .component_typeid_to_component_vec
            .get(&TypeId::of::<ComponentType>())
            .expect(
                "Type that tried to be fetched should exist
                         in archetype since types are fetched from this archetype originally.",
            );

        let source_component_vec_idx = *source_archetype.entity_to_component_index
            .get(&entity)
            .expect("Entity should have a component index tied to it since it is already established to be existing within the archetype");

        source_component_vec.remove(source_component_vec_idx);

        source_archetype.update_source_archetype_after_entity_move(entity);

        Ok(())
    }

    fn create_new_entity(&mut self) -> WorldResult<Entity> {
        let entity_id = self.entities.len();
        let entity = Entity {
            id: entity_id,
            //_generation: 0, /* todo(#53) update entity generation after an entity has been removed and then added. */
        };
        self.entities.push(entity);
        self.store_entity_in_archetype(entity, EMPTY_ENTITY_ARCHETYPE_INDEX)?;
        Ok(entity)
    }

    fn store_entity_in_archetype(
        &mut self,
        entity: Entity,
        archetype_index: ArchetypeIndex,
    ) -> WorldResult<()> {
        let archetype = self
            .archetypes
            .get_mut(archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(archetype_index))?;

        archetype
            .store_entity(entity)
            .map_err(WorldError::CouldNotAddComponent)?;

        self.entity_to_archetype_index
            .insert(entity, archetype_index);
        Ok(())
    }

    /// Returns the indices of all archetypes that at least contain the given signature.
    ///
    /// An example: if there exists the archetypes: (A), (A,B), (B,C), (A,B,C)
    /// and the signature (A,B) is given, the indices for archetypes: (A,B) and
    /// (A,B,C) will be returned as they both contain (A,B), while (A) only
    /// contains A components and no B components and (B,C) only contain B and C
    /// components and no A components.
    fn get_archetype_indices(
        &self,
        signature: &[TypeId],
    ) -> HashSet<ArchetypeIndex, BuildHasherDefault<NoHashHasher<usize>>> {
        let all_archetypes_with_signature_types: WorldResult<
            Vec<HashSet<ArchetypeIndex, BuildHasherDefault<NoHashHasher<usize>>>>,
        > = signature
            .iter()
            .map(|component_typeid| {
                self.component_typeid_to_archetype_indices
                    .get(component_typeid)
                    .cloned()
                    .ok_or(WorldError::ComponentTypeDoesNotExist(*component_typeid))
            })
            .collect();

        match all_archetypes_with_signature_types {
            Ok(archetype_indices) => intersection_of_multiple_sets(&archetype_indices),
            Err(_) => HashSet::default(),
        }
    }

    fn get_archetypes(&self, archetype_indices: &[ArchetypeIndex]) -> WorldResult<Vec<&Archetype>> {
        let archetypes: Result<Vec<_>, _> = archetype_indices
            .iter()
            .map(|&archetype_index| {
                self.archetypes
                    .get(archetype_index)
                    .ok_or(WorldError::ArchetypeDoesNotExist(archetype_index))
            })
            .collect();
        archetypes
    }

    fn get_exactly_matching_archetype(
        &self,
        component_type_ids: &Vec<TypeId>,
    ) -> Option<ArchetypeIndex> {
        if component_type_ids.is_empty() {
            return Some(EMPTY_ENTITY_ARCHETYPE_INDEX);
        }
        let mut sorted_component_type_ids = component_type_ids.clone();
        sorted_component_type_ids.sort();
        self.component_typeids_set_to_archetype_index
            .get(&sorted_component_type_ids)
            .copied()

        // Get_archetype_indices  returns all archetypes that contain
        // all matching component ids sent as input and no more. Minimum number of components
        // types contained within an archetype that is returned is therefore equal
        // to number of component types as the input. Checking for length should therefore
        // be enough if get_archetype_indices returns archetypes containing all
        // specified component types.
        /*let arch_ids = self.get_archetype_indices(component_type_ids);

        let contains_matching_components = |&arch_id: &ArchetypeIndex| {
            let archetype = self
                .archetypes
                .get(arch_id)
                .expect("Archetype should exist since index was just fetched.");

            archetype.component_typeid_to_component_vec.len() == component_type_ids.len()
        };
        arch_ids.into_iter().find(contains_matching_components)*/
    }

    fn borrow_component_vecs<ComponentType: Debug + Send + Sync + 'static>(
        &self,
        archetype_indices: &[ArchetypeIndex],
    ) -> WorldResult<Vec<ReadComponentVec<ComponentType>>> {
        let archetypes = self.get_archetypes(archetype_indices)?;

        let component_vecs = archetypes
            .iter()
            .map(|&archetype| archetype.borrow_component_vec::<ComponentType>())
            .collect();

        Ok(component_vecs)
    }

    fn borrow_component_vecs_mut<ComponentType: Debug + Send + Sync + 'static>(
        &self,
        archetype_indices: &[ArchetypeIndex],
    ) -> WorldResult<Vec<WriteComponentVec<ComponentType>>> {
        let archetypes = self.get_archetypes(archetype_indices)?;

        let component_vecs = archetypes
            .iter()
            .map(|&archetype| archetype.borrow_component_vec_mut::<ComponentType>())
            .collect();

        Ok(component_vecs)
    }

    fn get_archetype_of_entity(&self, entity: Entity) -> WorldResult<&Archetype> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let source_archetype = self
            .archetypes
            .get(source_archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_index))?;

        Ok(source_archetype)
    }
}

/// Mutably borrow two *separate* elements from the given slice.
/// Panics when the indexes are equal or out of bounds.
#[inline(always)]
fn get_mut_at_two_indices<T>(
    first_index: usize,
    second_index: usize,
    items: &mut [T],
) -> (&mut T, &mut T) {
    assert_ne!(first_index, second_index);
    let split_at_index = if first_index < second_index {
        second_index
    } else {
        first_index
    };
    let (first_slice, second_slice) = items.split_at_mut(split_at_index);
    if first_index < second_index {
        (&mut first_slice[first_index], &mut second_slice[0])
    } else {
        (&mut second_slice[0], &mut first_slice[second_index])
    }
}

fn panic_locked_component_vec<ComponentType: 'static>() -> ! {
    let component_type_name = any::type_name::<ComponentType>();
    panic!(
        "Lock of ComponentVec<{}> is already taken!",
        component_type_name
    )
}

impl Default for World {
    fn default() -> Self {
        Self {
            archetypes: vec![Archetype::default()],
            component_typeid_to_archetype_indices: Default::default(),
            component_typeids_set_to_archetype_index: Default::default(),
            entities: Default::default(),
            entity_to_archetype_index: Default::default(),
        }
    }
}

fn create_raw_component_vec<ComponentType: Debug + Send + Sync + 'static>() -> Box<dyn ComponentVec>
{
    Box::<ComponentVecImpl<ComponentType>>::default()
}

fn borrow_component_vec<ComponentType: 'static>(
    component_vec: &dyn ComponentVec,
) -> ReadComponentVec<ComponentType> {
    if let Some(component_vec) = component_vec
        .as_any()
        .downcast_ref::<ComponentVecImpl<ComponentType>>()
    {
        // This method should only be called once the scheduler has verified
        // that component access can be done without contention.
        // Panicking helps us detect errors in the scheduling algorithm more quickly.
        return match component_vec.try_read() {
            Ok(component_vec) => Some(component_vec),
            Err(TryLockError::WouldBlock) => panic_locked_component_vec::<ComponentType>(),
            Err(TryLockError::Poisoned(_)) => panic!("Lock should not be poisoned!"),
        };
    }
    None
}

fn borrow_component_vec_mut<ComponentType: 'static>(
    component_vec: &dyn ComponentVec,
) -> WriteComponentVec<ComponentType> {
    if let Some(component_vec) = component_vec
        .as_any()
        .downcast_ref::<ComponentVecImpl<ComponentType>>()
    {
        // This method should only be called once the scheduler has verified
        // that component access can be done without contention.
        // Panicking helps us detect errors in the scheduling algorithm more quickly.
        return match component_vec.try_write() {
            Ok(component_vec) => Some(component_vec),
            Err(TryLockError::WouldBlock) => panic_locked_component_vec::<ComponentType>(),
            Err(TryLockError::Poisoned(_)) => panic!("Lock should not be poisoned!"),
        };
    }
    None
}

/*fn intersection_of_multiple_sets<T: Hash + Eq + Clone>(sets: &[FnvHashSet<T>]) -> FnvHashSet<T> {
    let element_overlaps_with_all_other_sets =
        move |element: &&T| sets[1..].iter().all(|set| set.contains(element));
    sets.get(0)
        .unwrap_or(&FnvHashSet::default())
        .iter()
        .filter(element_overlaps_with_all_other_sets)
        .cloned()
        .collect()
}*/
fn intersection_of_multiple_sets<T: IsEnabled + Hash + Eq + Clone>(
    sets: &[HashSet<T, BuildHasherDefault<NoHashHasher<T>>>],
) -> HashSet<T, BuildHasherDefault<NoHashHasher<T>>> {
    let element_overlaps_with_all_other_sets =
        move |element: &&T| sets[1..].iter().all(|set| set.contains(element));
    sets.get(0)
        .unwrap_or(&HashSet::default())
        .iter()
        .filter(element_overlaps_with_all_other_sets)
        .cloned()
        .collect()
}

type ComponentVecImpl<ComponentType> = RwLock<Vec<ComponentType>>;

trait ComponentVec: Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Returns the type stored in the component vector.
    fn stored_type(&self) -> TypeId;
    /// Returns the number of components stored in the component vector.
    fn len(&self) -> usize;
    /// Removes the entity from the component vector.
    fn remove(&self, index: usize);
    /// Removes a component and pushes it to another
    /// archetypes component vector of the same data type.
    fn move_element(&self, source_index: usize, target_arch: &mut Archetype)
        -> ArchetypeResult<()>;
}

impl<T: Debug + Send + Sync + 'static> ComponentVec for ComponentVecImpl<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn stored_type(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn len(&self) -> usize {
        Vec::len(&self.read().expect("Lock is poisoned"))
    }

    fn remove(&self, index: usize) {
        self.write().expect("Lock is poisoned").swap_remove(index);
    }

    fn move_element(
        &self,
        source_index: usize,
        target_arch: &mut Archetype,
    ) -> ArchetypeResult<()> {
        let value = self
            .write()
            .expect("Lock is poisoned")
            .swap_remove(source_index);

        if !target_arch.contains_component_type::<T>() {
            target_arch.add_component_vec::<T>()
        }

        let target_vec = target_arch
            .component_typeid_to_component_vec
            .get(&TypeId::of::<T>())
            .expect("component vec has been added if it was not there previously")
            .as_ref();

        let mut component_vec = borrow_component_vec_mut::<T>(target_vec)
            .ok_or(ArchetypeError::CouldNotBorrowComponentVec(TypeId::of::<T>()))?;

        component_vec.push(value);

        Ok(())
    }
}

/// An entity is an identifier that represents a simulated object consisting of multiple
/// different components.
#[derive(Default, Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone, Hash)]
pub struct Entity {
    id: usize,
    //_generation: usize,
}

impl IsEnabled for Entity {}

#[cfg(test)]
mod tests {
    use super::systems::*;
    use super::*;
    use test_case::test_case;
    use test_log::test;

    #[derive(Debug)]
    struct A;

    #[derive(Debug)]
    struct B;

    #[derive(Debug)]
    struct C;

    trait BorrowComponentVecsWithSignature {
        fn borrow_component_vecs_with_signature<ComponentType: Debug + Send + Sync + 'static>(
            &self,
            signature: &[TypeId],
        ) -> WorldResult<Vec<ReadComponentVec<ComponentType>>>;

        fn borrow_component_vecs_with_signature_mut<ComponentType: Debug + Send + Sync + 'static>(
            &self,
            signature: &[TypeId],
        ) -> WorldResult<Vec<WriteComponentVec<ComponentType>>>;
    }

    impl BorrowComponentVecsWithSignature for World {
        fn borrow_component_vecs_with_signature<ComponentType: Debug + Send + Sync + 'static>(
            &self,
            signature: &[TypeId],
        ) -> WorldResult<Vec<ReadComponentVec<ComponentType>>> {
            let archetype_indices: Vec<_> =
                self.get_archetype_indices(signature).into_iter().collect();
            self.borrow_component_vecs(&archetype_indices)
        }

        fn borrow_component_vecs_with_signature_mut<
            ComponentType: Debug + Send + Sync + 'static,
        >(
            &self,
            signature: &[TypeId],
        ) -> WorldResult<Vec<WriteComponentVec<ComponentType>>> {
            let archetype_indices: Vec<_> =
                self.get_archetype_indices(signature).into_iter().collect();
            self.borrow_component_vecs_mut(&archetype_indices)
        }
    }

    #[test]
    #[should_panic(expected = "Lock of ComponentVec<ecs::tests::A> is already taken!")]
    fn world_panics_when_trying_to_mutably_borrow_same_components_twice() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap();
        world.add_component_to_entity(entity, A).unwrap();

        let _first = world
            .borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()])
            .unwrap();
        let _second = world
            .borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()])
            .unwrap();
    }

    #[test]
    fn world_doesnt_panic_when_mutably_borrowing_components_after_dropping_previous_mutable_borrow()
    {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap();

        world.add_component_to_entity(entity, A).unwrap();

        let first = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
        drop(first);
        let _second = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
    }

    #[test]
    fn world_does_not_panic_when_trying_to_immutably_borrow_same_components_twice() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap();

        world.add_component_to_entity(entity, A).unwrap();

        let _first = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
        let _second = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
    }

    // Archetype tests:

    #[test]
    fn type_id_order_does_not_affects_fetching_of_correct_archetype() {
        let (world, _, _, _, _) = setup_world_with_3_entities_with_u32_and_i32_components();

        let type_vector_1 = vec![TypeId::of::<u32>(), TypeId::of::<i32>()];
        let type_vector_2 = vec![TypeId::of::<i32>(), TypeId::of::<u32>()];
        let result_1 = world
            .get_exactly_matching_archetype(&type_vector_1)
            .unwrap();
        let result_2 = world
            .get_exactly_matching_archetype(&type_vector_2)
            .unwrap();

        assert_eq!(result_1, result_2);
    }

    #[test]
    fn entities_change_archetype_after_component_addition() {
        let mut world = World::default();
        let entity = world.create_new_entity().unwrap();

        world.add_component_to_entity(entity, A).unwrap();

        let mut actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 1);

        world.add_component_to_entity(entity, B).unwrap();

        actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 2);
    }

    #[test]
    fn entities_change_archetype_after_component_removal() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap();

        world.add_component_to_entity(entity, A).unwrap();

        let mut actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 1);

        world
            .remove_component_type_from_entity::<A>(entity)
            .unwrap();

        actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 0);
    }

    #[test]
    fn last_entity_in_archetype_moves_between_archetypes_as_expected() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap(); // Arch 0

        world.add_component_to_entity(entity, A).unwrap(); // Arch 1
        world.add_component_to_entity(entity, B).unwrap(); // Arch 2

        let mut actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 2);

        world
            .remove_component_type_from_entity::<A>(entity)
            .unwrap(); // Arch 3

        actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 3);

        world
            .remove_component_type_from_entity::<B>(entity)
            .unwrap(); // Arch 0

        actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 0);
    }

    fn setup_world_with_3_entities_with_u32_and_i32_components(
    ) -> (World, usize, Entity, Entity, Entity) {
        let mut world = World::default();

        let entity1 = world.create_new_entity().unwrap();
        let entity2 = world.create_new_entity().unwrap();
        let entity3 = world.create_new_entity().unwrap();

        world.add_component_to_entity(entity1, 1_u32).unwrap();
        world.add_component_to_entity(entity1, 1_i32).unwrap();

        world.add_component_to_entity(entity2, 2_u32).unwrap();
        world.add_component_to_entity(entity2, 2_i32).unwrap();

        world.add_component_to_entity(entity3, 3_u32).unwrap();
        world.add_component_to_entity(entity3, 3_i32).unwrap();

        // All entities in archetype with index 2 now
        (world, 2, entity1, entity2, entity3)
    }

    type UnwrappedReadComponentVec<'a, ComponentType> = RwLockReadGuard<'a, Vec<ComponentType>>;

    fn get_u32_and_i32_component_vectors(
        archetype: &Archetype,
    ) -> (
        UnwrappedReadComponentVec<u32>,
        UnwrappedReadComponentVec<i32>,
    ) {
        let component_vec_u32 = archetype
            .component_typeid_to_component_vec
            .get(&TypeId::of::<u32>())
            .unwrap()
            .as_ref();
        let u32_read_vec = borrow_component_vec::<u32>(component_vec_u32);

        let component_vec_i32 = archetype
            .component_typeid_to_component_vec
            .get(&TypeId::of::<i32>())
            .unwrap()
            .as_ref();
        let i32_read_vec = borrow_component_vec::<i32>(component_vec_i32);

        (u32_read_vec.unwrap(), i32_read_vec.unwrap())
    }

    #[test]
    fn entities_contain_correct_values_after_adding_components() {
        let (world, relevant_archetype_index, _, _, _) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let (u32_read_vec, i32_read_vec) = get_u32_and_i32_component_vectors(archetype);
        let u32_values = u32_read_vec;
        let i32_values = i32_read_vec;

        assert_eq!(&[1_u32, 2_u32, 3_u32], &u32_values[..]);
        assert_eq!(&[1_i32, 2_i32, 3_i32], &i32_values[..]);
    }

    #[test]
    fn entities_are_in_expected_order_according_to_when_components_were_added() {
        let (world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        assert_eq!(archetype.entity_order, vec![entity1, entity2, entity3]);
    }

    #[test]
    fn entity_to_component_index_gives_expected_values_after_addition() {
        let (world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let entity1_component_index = *archetype.entity_to_component_index.get(&entity1).unwrap();
        let entity2_component_index = *archetype.entity_to_component_index.get(&entity2).unwrap();
        let entity3_component_index = *archetype.entity_to_component_index.get(&entity3).unwrap();

        assert_eq!(entity1_component_index, 0);
        assert_eq!(entity2_component_index, 1);
        assert_eq!(entity3_component_index, 2);
    }

    #[test]
    fn entity_values_are_removed_from_archetype_after_addition() {
        let (mut world, relevant_archetype_index, entity1, _, _) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let (u32_read_vec, i32_read_vec) = get_u32_and_i32_component_vectors(archetype);
        let u32_values = u32_read_vec;
        let i32_values = i32_read_vec;

        assert!(u32_values.get(2).is_none());
        assert!(i32_values.get(2).is_none());
    }

    #[test]
    fn values_swap_index_after_entity_has_been_moved_by_addition() {
        let (mut world, relevant_archetype_index, entity1, _, _) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let (u32_read_vec, i32_read_vec) = get_u32_and_i32_component_vectors(archetype);
        let u32_values = u32_read_vec;
        let i32_values = i32_read_vec;

        assert_eq!(&u32_values[..2], &[3_u32, 2_u32]);
        assert_eq!(&i32_values[..2], &[3_i32, 2_i32]);
    }

    #[test]
    fn entity_order_swap_index_after_entity_has_been_moved_by_addition() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        assert_eq!(archetype.entity_order, vec![entity3, entity2]);
    }

    #[test]
    fn entity_to_component_index_is_updated_after_move_by_addition() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let entity1_component_index = archetype.entity_to_component_index.get(&entity1);
        let entity2_component_index = archetype.entity_to_component_index.get(&entity2);
        let entity3_component_index = archetype.entity_to_component_index.get(&entity3);
        assert!(entity1_component_index.is_none());
        assert_eq!(entity2_component_index, Some(&1));
        assert_eq!(entity3_component_index, Some(&0));
    }

    #[test]
    fn entities_are_added_to_worlds_entity_list() {
        let (mut world, _, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        assert_eq!(world.entities, vec![entity1, entity2, entity3]);
    }

    #[test]
    fn entity_to_archetype_index_is_updated_correctly_after_move_by_addition() {
        let (mut world, _, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let entity1_archetype_index = *world.entity_to_archetype_index.get(&entity1).unwrap();
        let entity2_archetype_index = *world.entity_to_archetype_index.get(&entity2).unwrap();
        let entity3_archetype_index = *world.entity_to_archetype_index.get(&entity3).unwrap();
        assert_eq!(entity1_archetype_index, 3_usize);
        assert_eq!(entity2_archetype_index, 2_usize);
        assert_eq!(entity3_archetype_index, 2_usize);
    }

    #[test]
    fn entities_contain_correct_values_after_removing_components() {
        let (mut world, relevant_archetype_index, entity1, _, _) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype_2 = world.archetypes.get(relevant_archetype_index).unwrap();

        let archetype_3 = world.archetypes.get(3).unwrap();

        let arch_3_i32_values = archetype_3.borrow_component_vec::<i32>().unwrap();

        let (u32_read_vec, i32_read_vec) = get_u32_and_i32_component_vectors(archetype_2);
        let arch_2_u32_values = u32_read_vec;
        let arch_2_i32_values = i32_read_vec;

        assert_eq!([3_u32, 2_u32], arch_2_u32_values[..]);
        assert_eq!([3_i32, 2_i32], arch_2_i32_values[..]);
        assert_eq!([1_i32], arch_3_i32_values[..]);
    }

    #[test]
    fn entity_to_component_index_gives_expected_values_after_removal() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype_1 = world.archetypes.get(3).unwrap();
        let archetype_2 = world.archetypes.get(relevant_archetype_index).unwrap();

        let arch_1_entity1_component_index = archetype_1.entity_to_component_index.get(&entity1);

        let arch_2_entity1_component_index = archetype_2.entity_to_component_index.get(&entity1);
        let arch_2_entity2_component_index = archetype_2.entity_to_component_index.get(&entity2);
        let arch_2_entity3_component_index = archetype_2.entity_to_component_index.get(&entity3);

        assert_eq!(arch_1_entity1_component_index, Some(&0));
        assert_eq!(arch_2_entity1_component_index, None);
        assert_eq!(arch_2_entity2_component_index, Some(&1));
        assert_eq!(arch_2_entity3_component_index, Some(&0));
    }

    #[test]
    fn entity_values_are_removed_from_archetype_after_removal() {
        let (mut world, relevant_archetype_index, entity1, _, _) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let (u32_read_vec, i32_read_vec) = get_u32_and_i32_component_vectors(archetype);
        let u32_values = u32_read_vec;
        let i32_values = i32_read_vec;

        assert!(u32_values.get(2).is_none());
        assert!(i32_values.get(2).is_none());
    }

    #[test]
    fn values_swap_index_after_entity_has_been_moved_by_removal() {
        let (mut world, relevant_archetype_index, entity1, _, _) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let (u32_read_vec, i32_read_vec) = get_u32_and_i32_component_vectors(archetype);
        let u32_values = u32_read_vec;
        let i32_values = i32_read_vec;

        assert_eq!(&u32_values[..2], &[3_u32, 2_u32]);
        assert_eq!(&i32_values[..2], &[3_i32, 2_i32]);
    }

    #[test]
    fn entity_order_swap_index_after_entity_has_been_moved_by_removal() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        assert_eq!(archetype.entity_order, vec![entity3, entity2]);
    }

    #[test]
    fn entity_to_component_index_is_updated_after_move_by_removal() {
        let (mut world, relevant_archetype_index, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let entity1_component_index = archetype.entity_to_component_index.get(&entity1);
        let entity2_component_index = archetype.entity_to_component_index.get(&entity2);
        let entity3_component_index = archetype.entity_to_component_index.get(&entity3);
        assert!(entity1_component_index.is_none());
        assert_eq!(entity2_component_index, Some(&1));
        assert_eq!(entity3_component_index, Some(&0));
    }

    #[test]
    fn entity_to_archetype_index_is_updated_correctly_after_move_by_removal() {
        let (mut world, _, entity1, entity2, entity3) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Remove component from entity1 causing it to move to Arch_3
        world
            .remove_component_type_from_entity::<u32>(entity1)
            .unwrap();

        let entity1_archetype_index = *world.entity_to_archetype_index.get(&entity1).unwrap();
        let entity2_archetype_index = *world.entity_to_archetype_index.get(&entity2).unwrap();
        let entity3_archetype_index = *world.entity_to_archetype_index.get(&entity3).unwrap();
        assert_eq!(entity1_archetype_index, 3_usize);
        assert_eq!(entity2_archetype_index, 2_usize);
        assert_eq!(entity3_archetype_index, 2_usize);
    }

    #[test]
    #[should_panic]
    fn error_when_adding_existing_component_type_to_entity() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap(); // arch_0

        let component_1: usize = 10;
        let component_2: usize = 20;

        world.add_component_to_entity(entity, component_1).unwrap();
        world.add_component_to_entity(entity, component_2).unwrap();
    }

    #[test]
    #[should_panic]
    fn error_when_removing_nonexistent_component_type_to_entity() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap(); // arch_0

        world
            .remove_component_type_from_entity::<i32>(entity)
            .unwrap();
    }

    #[test]
    fn entities_maintain_component_values_after_moving() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap(); // arch_0

        let component_1: usize = 10;
        let component_2: f32 = 123_f32;
        let component_3: i64 = 321;

        world.add_component_to_entity(entity, component_1).unwrap(); // arch_1
        world.add_component_to_entity(entity, component_2).unwrap(); // arch_2
        world.add_component_to_entity(entity, component_3).unwrap(); // arch_3

        world
            .remove_component_type_from_entity::<f32>(entity)
            .unwrap(); // arch_4

        // fetch values from arch_4
        let archetype_4: &Archetype = world.archetypes.get(4).unwrap();

        let component_vec_4_usize = archetype_4.borrow_component_vec::<usize>().unwrap();
        let value_usize = component_vec_4_usize.first().unwrap();

        let component_vec_4_i64 = archetype_4.borrow_component_vec::<i64>().unwrap();
        let value_i64 = component_vec_4_i64.first().unwrap();

        assert_eq!(*value_usize, 10);
        assert_eq!(*value_i64, 321);
    }

    fn setup_world_with_three_entities_and_components() -> World {
        // Arrange
        let mut world = World::default();

        let entity1 = world.create_new_entity().unwrap();
        let entity2 = world.create_new_entity().unwrap();
        let entity3 = world.create_new_entity().unwrap();

        // insert some components...
        world.add_component_to_entity::<u64>(entity1, 1).unwrap();
        world.add_component_to_entity::<u32>(entity1, 2).unwrap();

        world.add_component_to_entity::<u64>(entity2, 3).unwrap();
        world.add_component_to_entity::<u32>(entity2, 4).unwrap();

        world.add_component_to_entity::<u32>(entity3, 6).unwrap();

        world
    }

    #[test]
    fn borrow_with_signature_returns_expected_values() {
        // Arrange
        let world = setup_world_with_three_entities_and_components();

        // Act
        // Borrow all vecs containing u32 from archetypes have the signature u32
        let vecs_u32 = world
            .borrow_component_vecs_with_signature::<u32>(&[TypeId::of::<u32>()])
            .unwrap();
        eprintln!("vecs_u32 = {vecs_u32:#?}");
        // Assert
        // Collect values from vecs
        let result: HashSet<u32> = vecs_u32
            .iter()
            .flat_map(|component_vec| component_vec.as_ref().unwrap().iter().copied())
            .collect();
        println!("{result:?}");

        assert_eq!(result, HashSet::from([2, 4, 6]))
    }

    #[test]
    fn borrowing_non_existent_component_returns_empty_vec() {
        // Arrange
        let world = setup_world_with_three_entities_and_components();

        // Act
        let vecs_f32 = world
            .borrow_component_vecs_with_signature::<f32>(&[TypeId::of::<f32>()])
            .unwrap();
        eprintln!("vecs_f32 = {vecs_f32:#?}");
        // Assert
        // Collect values from vecs
        let result: Vec<f32> = vecs_f32
            .iter()
            .flat_map(|component_vec| component_vec.as_ref().unwrap().iter().copied())
            .collect();
        println!("{result:?}");

        assert_eq!(result, vec![])
    }

    #[test]
    fn querying_with_archetypes() {
        // Arrange
        let world = setup_world_with_three_entities_and_components();

        let mut result: HashSet<u32> = HashSet::new();

        let archetypes: Vec<ArchetypeIndex> = world
            .get_archetype_indices(&[TypeId::of::<u32>()])
            .into_iter()
            .collect();

        let mut borrowed = <Read<u32> as SystemParameter>::borrow(&world, &archetypes).unwrap();

        // SAFETY: This is safe because the result from fetch_parameter will not outlive borrowed
        unsafe {
            while let Some(parameter) =
                <Read<u32> as SystemParameter>::fetch_parameter(&mut borrowed)
            {
                result.insert(*parameter);
            }
        }

        assert_eq!(result, HashSet::from([2, 4, 6]))
    }

    #[test]
    #[should_panic]
    fn borrowing_component_vec_twice_from_archetype_causes_panic() {
        let mut archetype = Archetype::default();
        archetype.add_component_vec::<u32>();

        let borrow_1 = archetype.borrow_component_vec_mut::<u32>();
        let borrow_2 = archetype.borrow_component_vec_mut::<u32>();

        // Drop after both have been borrowed to make sure they both live this long.
        drop(borrow_1);
        drop(borrow_2);
    }

    #[test]
    fn borrowing_component_vec_after_reference_has_been_dropped_does_not_cause_panic() {
        let mut archetype = Archetype::default();
        archetype.add_component_vec::<u32>();

        let borrow_1 = archetype.borrow_component_vec_mut::<u32>();
        drop(borrow_1);

        let borrow_2 = archetype.borrow_component_vec_mut::<u32>();
        drop(borrow_2);
    }

    #[test]
    fn borrowing_two_different_component_vecs_from_archetype_does_not_cause_panic() {
        let mut archetype = Archetype::default();
        archetype.add_component_vec::<u32>();
        archetype.add_component_vec::<u64>();

        let a = archetype.borrow_component_vec_mut::<u32>();
        let b = archetype.borrow_component_vec_mut::<u64>();

        // Drop after both have been borrowed to make sure they both live this long.
        drop(a);
        drop(b);
    }

    // Intersection tests:
    #[test_case(vec![vec![1,2,3]], vec![1,2,3]; "self intersection")]
    #[test_case(vec![vec![1,2,3], vec![1,2,3]], vec![1,2,3]; "two of the same")]
    #[test_case(vec![vec![1], vec![2,3], vec![4]], vec![]; "no overlap, no matches")]
    #[test_case(vec![vec![1,2], vec![2,3], vec![3,4]], vec![]; "some overlap, no matches")]
    #[test_case(vec![vec![1,2,3,4], vec![2,3], vec![3,4]], vec![3]; "some matches")]
    #[test_case(vec![vec![]], vec![]; "empty")]
    #[test_case(vec![vec![], vec![], vec![], vec![]], vec![]; "multiple empty")]
    #[test_case(vec![vec![], vec![1,2,3,4]], vec![]; "one empty, one not")]
    #[test_case(vec![vec![]], vec![]; "all empty")]
    #[test_case(vec![vec![2,1,1,1,1], vec![1,1,1,1,2], vec![1,1,2,1,1]], vec![2,1,1,1,1]; "multiple of the same number")]
    fn intersection_returns_expected_values(
        test_vecs: Vec<Vec<usize>>,
        expected_value: Vec<usize>,
    ) {
        // Construct test values, to avoid upsetting Rust and test_case
        let borrowed_test_vecs: Vec<HashSet<usize, BuildHasherDefault<NoHashHasher<usize>>>> =
            test_vecs
                .iter()
                .map(|vec| HashSet::from_iter(vec.clone()))
                .collect();
        let borrowed_expected_value: HashSet<usize, BuildHasherDefault<NoHashHasher<usize>>> =
            expected_value.into_iter().collect();

        // Perform intersection operation
        let result = intersection_of_multiple_sets(&borrowed_test_vecs);

        // Assert intersection result equals expected value
        assert_eq!(result, borrowed_expected_value);
    }
}
