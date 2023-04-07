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
use std::any;
use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::Deref;
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
    world: World,
    systems: Vec<Box<dyn System>>,
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
    /// Executes systems in a world according to a given schedule.
    fn execute<S: Schedule<'systems>>(
        &mut self,
        schedule: S,
        world: &'systems World,
        shutdown_receiver: Receiver<()>,
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
            for batch in schedule
                .currently_executable_systems()
                .map_err(ExecutionError::Schedule)?
            {
                batch.run(world).map_err(ExecutionError::System)?;
            }
        }
        Ok(())
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
}

/// Whether a schedule operation succeeded.
pub type ScheduleResult<T, E = ScheduleError> = Result<T, E>;

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
    fn currently_executable_systems(
        &mut self,
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
pub struct Unordered<'systems>(&'systems [Box<dyn System>]);

impl<'systems> Schedule<'systems> for Unordered<'systems> {
    fn generate(systems: &'systems [Box<dyn System>]) -> ScheduleResult<Self> {
        Ok(Self(systems))
    }

    fn currently_executable_systems(
        &mut self,
    ) -> ScheduleResult<Vec<SystemExecutionGuard<'systems>>> {
        Ok(self
            .0
            .iter()
            .map(|system| system.as_ref())
            .map(|system| SystemExecutionGuard::create(system).0)
            .collect())
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
}

/// Whether a archetype operation succeeded.
pub type ArchetypeResult<T, E = ArchetypeError> = Result<T, E>;

/// Stores components associated with entity ids.
#[derive(Debug, Default)]
struct Archetype {
    component_typeid_to_component_vec: HashMap<TypeId, Box<dyn ComponentVec>>,
    entity_to_component_index: HashMap<Entity, ComponentIndex>,
    //last_entity_added: Entity,
    //component_index_to_entity: HashMap<ComponentIndex, Entity>,
    entity_order: Vec<Entity>,
}

impl Archetype {
    /// Adds an `entity_id` to keep track of and store components for.
    ///
    /// The function is idempotent when passing the same `id` multiple times.
    fn store_entity(&mut self, entity: Entity) {
        if !self.entity_to_component_index.contains_key(&entity) {
            let entity_index = self.entity_to_component_index.len();

            self.entity_to_component_index.insert(entity, entity_index);

            //self.component_index_to_entity.insert(entity_index, entity);
            self.entity_order.push(entity)
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
        // new
        let mut component_vec = self.borrow_component_vec_mut::<ComponentType>().ok_or(
            ArchetypeError::CouldNotBorrowComponentVec(TypeId::of::<ComponentType>()),
        )?;
        component_vec.push(Some(component));

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
}

/// An error occurred during a world operation.
#[derive(Error, Debug)]
pub enum WorldError {
    /// Could not find archetype with the given index.
    #[error("could not find archetype with the given index: {0:?}")]
    ArchetypeDoesNotExist(ArchetypeIndex),
    /// Could not add component to archetype with the given index.
    #[error("could not add component to archetype with the given index: {0:?}")]
    CouldNotAddComponent(ArchetypeError),
    /// Could not move component to archetype with the given index.
    #[error("could not move component to archetype with the given index: {0:?}")]
    CouldNotMoveComponent(ArchetypeError),
    /// Could not find the given component type.
    #[error("could not find the given component type: {0:?}")]
    ComponentTypeDoesNotExist(TypeId),
    /// Could not find the given entity id.
    #[error("could not find the given entity: {0:?}")]
    EntityDoesNotExist(Entity),
    /// Component of same type already exists for entity
    #[error("component of same type {1:?} already exists for entity {0:?}")]
    ComponentTypeAlreadyExistsForEntity(Entity, TypeId),
    /// Component of same type already exists for entity
    #[error("component of type {1:?} does not exists for entity {0:?}")]
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
    entity_to_archetype_index: HashMap<Entity, ArchetypeIndex>,
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
    component_typeid_to_archetype_indices: HashMap<TypeId, HashSet<ArchetypeIndex>>,
}

type ReadComponentVec<'a, ComponentType> = Option<RwLockReadGuard<'a, Vec<Option<ComponentType>>>>;
type WriteComponentVec<'a, ComponentType> =
    Option<RwLockWriteGuard<'a, Vec<Option<ComponentType>>>>;
type TargetArchSourceTargetIDs = (Option<ArchetypeIndex>, (Vec<TypeId>, Vec<TypeId>));

impl World {
    fn find_target_archetype<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        removal: bool,
    ) -> WorldResult<TargetArchSourceTargetIDs> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let source_archetype = self
            .archetypes
            .get(source_archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_index))?;

        // Find new archetype
        let mut source_type_ids: Vec<TypeId> = vec![];
        let mut target_type_ids: Vec<TypeId> = vec![];
        let mut type_to_be_removed_exists_in_arch = false;

        // Get components which should be present in new archetype
        source_archetype
            .component_typeid_to_component_vec
            .keys()
            .cloned()
            .for_each(|type_id| {
                source_type_ids.push(type_id);
                // Do not add component type that is supposed to be removed
                if removal && type_id == TypeId::of::<ComponentType>() {
                    type_to_be_removed_exists_in_arch = true;
                } else {
                    target_type_ids.push(type_id);
                }
            });

        if !removal {
            target_type_ids.push(TypeId::of::<ComponentType>());
        }

        // Return error if component we wish to remove does not exist within old archetype.
        if removal && !type_to_be_removed_exists_in_arch {
            return Err(WorldError::ComponentTypeNotPresentForEntity(
                entity,
                TypeId::of::<ComponentType>(),
            ));
        } else if !removal && source_type_ids.contains(&TypeId::of::<ComponentType>()) {
            return Err(WorldError::ComponentTypeAlreadyExistsForEntity(
                entity,
                TypeId::of::<ComponentType>(),
            ));
        }

        let target_archetype_exists = self.get_archetype_with_matching_components(&target_type_ids);

        Ok((target_archetype_exists, (source_type_ids, target_type_ids)))
    }

    fn move_entity_components_between_archetypes(
        &mut self,
        entity: Entity,
        target_archetype_idx: ArchetypeIndex,
        components_to_move: Vec<TypeId>,
    ) -> WorldResult<()> {
        let source_archetype_index = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        let source_archetype = self
            .archetypes
            .get(source_archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_index))?;

        let source_component_vec_idx = *self
            .archetypes
            .get(source_archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_index))?.entity_to_component_index
            .get(&entity)
            .expect("Entity should have a component index tied to it since it is already established to be existing within the archetype");

        // move_element removes component from source but does not move if no ComponentVec
        // with matching type exists in target
        for type_id in components_to_move {
            source_archetype.component_typeid_to_component_vec
                .get(&type_id)
                .expect("Type that tried to be fetched should exist
                     in archetype since types are fetched from this archetype originally.")
                .move_element(source_component_vec_idx,
                              self.archetypes
                                  .get(target_archetype_idx)
                                  .expect("Archetype should exist since it was the requirement for entering this matching case"))
                .map_err(WorldError::CouldNotMoveComponent)?;
        }

        let target_archetype: &mut Archetype = self
            .archetypes
            .get_mut(target_archetype_idx)
            .expect(
            "Archetype should exist since it was the requirement for entering this matching case",
        );

        target_archetype
            .entity_to_component_index
            .insert(entity, target_archetype.entity_to_component_index.len());

        target_archetype.entity_order.push(entity);
        self.entity_to_archetype_index
            .insert(entity, target_archetype_idx);

        Ok(())
    }

    fn move_entity_components_to_new_archetype(
        &mut self,
        entity: Entity,
        components_to_move: Vec<TypeId>,
    ) -> WorldResult<ArchetypeIndex> {
        // Create new archetype and move existing components there
        let mut target_archetype = Archetype::default();
        let target_archetype_idx = self.archetypes.len();
        let source_archetype_idx = *self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;
        let source_archetype = self
            .archetypes
            .get(source_archetype_idx)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_idx))?;
        let source_component_vec_idx = *source_archetype
            .entity_to_component_index
            .get(&entity)
            .ok_or(WorldError::EntityDoesNotExist(entity))?;

        for type_id in components_to_move {
            source_archetype
                .component_typeid_to_component_vec
                .get(&type_id)
                .expect(
                    "Type that tried to be fetched should exist
                            in archetype since types are fetched from this archetype originally.",
                )
                .move_element_to_new(source_component_vec_idx, &mut target_archetype)
                .map_err(WorldError::CouldNotMoveComponent)?;

            self.component_typeid_to_archetype_indices
                .get_mut(&type_id)
                .expect("Type ID should exist for previously existing types")
                .insert(target_archetype_idx);
        }

        target_archetype.entity_order.push(entity);
        target_archetype.entity_to_component_index.insert(entity, 0);
        self.archetypes.push(target_archetype);
        self.entity_to_archetype_index
            .insert(entity, target_archetype_idx);

        Ok(target_archetype_idx)
    }

    fn update_source_archetype_after_entity_move(
        &mut self,
        entity: Entity,
        source_archetype_index: ArchetypeIndex,
        source_component_vec_idx: ComponentIndex,
    ) -> WorldResult<()> {
        let source_archetype = self
            .archetypes
            .get_mut(source_archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_index))?;

        // Update old archetypes component vec index after moving entity
        if let Some(&swapped_entity) = source_archetype.entity_order.last() {
            if swapped_entity != entity {
                source_archetype
                    .entity_to_component_index
                    .insert(swapped_entity, source_component_vec_idx);
            }
        }
        // Remove entity component index existing in its old archetype
        source_archetype
            .entity_order
            .swap_remove(source_component_vec_idx);

        source_archetype.entity_to_component_index.remove(&entity);
        Ok(())
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

        let source_component_vec_idx = *self
            .archetypes
            .get(source_archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_index))?
            .entity_to_component_index
            .get(&entity)
            .expect(
                "Entity should yield a component index
             in archetype since the the relevant archetype
              was fetched from the entity itself.",
            );

        let (target_archetype_exists, (source_type_ids, _)) =
            self.find_target_archetype::<ComponentType>(entity, false)?;

        // Handle if an archetype that contains only relevant components exists or not
        match target_archetype_exists {
            Some(target_archetype_idx) => {
                self.move_entity_components_between_archetypes(
                    entity,
                    target_archetype_idx,
                    source_type_ids,
                )?;

                // Add new component to target archetype
                self.archetypes
                    .get_mut(target_archetype_idx)
                    .expect("Archetype should exist since it was the requirement for entering this matching case")
                    .add_component(component)
                    .map_err(WorldError::CouldNotAddComponent)?;
            }
            None => {
                let target_archetype_idx =
                    self.move_entity_components_to_new_archetype(entity, source_type_ids)?;

                let target_archetype = self
                    .archetypes
                    .get_mut(target_archetype_idx)
                    .expect("This should be added in the previous function call");
                // Handle incoming component
                target_archetype.add_component_vec::<ComponentType>();
                target_archetype
                    .add_component(component)
                    .map_err(WorldError::CouldNotAddComponent)?;

                // Update component_type_id_to_archetype when the the incoming
                // component type had previously existed. Alternatively not existed.
                let type_id_of_added_component = TypeId::of::<ComponentType>();

                if let Some(archetype_indices) = self
                    .component_typeid_to_archetype_indices
                    .get_mut(&type_id_of_added_component)
                {
                    archetype_indices.insert(target_archetype_idx);
                } else {
                    let mut new_archetype_indices = HashSet::new();
                    new_archetype_indices.insert(target_archetype_idx);
                    self.component_typeid_to_archetype_indices
                        .insert(type_id_of_added_component, new_archetype_indices);
                }
            }
        }

        self.update_source_archetype_after_entity_move(
            entity,
            source_archetype_index,
            source_component_vec_idx,
        )?;

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

        let source_component_vec_idx = *self
            .archetypes
            .get(source_archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(source_archetype_index))?.entity_to_component_index
            .get(&entity)
            .expect("Entity should have a component index tied to it since it is already established to be existing within the archetype");

        let (target_archetype_exists, (source_type_ids, target_type_ids)) =
            self.find_target_archetype::<ComponentType>(entity, true)?;

        // Handle if an archetype that contains only relevant components exists or not
        match target_archetype_exists {
            Some(target_archetype_idx) => {
                self.move_entity_components_between_archetypes(
                    entity,
                    target_archetype_idx,
                    source_type_ids,
                )?;
            }
            None => {
                let target_archetype_idx =
                    self.move_entity_components_to_new_archetype(entity, target_type_ids)?;

                // Remove component
                let source_archetype = self
                    .archetypes
                    .get(source_archetype_index)
                    .expect("Should exist since it existed in the begining of the method");

                let target_archetype = self
                    .archetypes
                    .get(target_archetype_idx)
                    .expect("Archetype should have been created by previous function");
                source_archetype
                    .component_typeid_to_component_vec
                    .get(&TypeId::of::<ComponentType>())
                    .expect(
                        "Type that tried to be fetched should exist
                         in archetype since types are fetched from this archetype originally.",
                    )
                    .move_element(source_component_vec_idx, target_archetype)
                    .map_err(WorldError::CouldNotMoveComponent)?;
            }
        }

        self.update_source_archetype_after_entity_move(
            entity,
            source_archetype_index,
            source_component_vec_idx,
        )?;

        Ok(())
    }

    fn create_new_entity(&mut self) -> WorldResult<Entity> {
        let entity_id = self.entities.len();
        let entity = Entity {
            id: entity_id,
            _generation: 0, /* todo(#53) update entity generation after an entity has been removed and then added. */
        };
        self.entities.push(entity);
        self.store_entity_in_archetype(entity, 0)?;
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

        archetype.store_entity(entity);

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
    fn get_archetype_indices(&self, signature: &[TypeId]) -> HashSet<ArchetypeIndex> {
        let all_archetypes_with_signature_types: WorldResult<Vec<HashSet<ArchetypeIndex>>> =
            signature
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
            Err(_) => HashSet::new(),
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

    fn get_archetype_with_matching_components(
        &self,
        component_type_ids: &Vec<TypeId>,
    ) -> Option<ArchetypeIndex> {
        if component_type_ids.is_empty() {
            return Some(0);
        }
        // Get all archetypes containing all the component types
        let arch_ids = self.get_archetype_indices(component_type_ids);
        let number_of_components = component_type_ids.len();

        // Find if archetype containing only relevant components exists
        arch_ids.into_iter().find(|&arch_id| {
            self.archetypes
                .get(arch_id)
                .expect("Archetype should exist since index was just fetched.")
                .component_typeid_to_component_vec
                .len()
                == number_of_components
        })
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
            entities: Default::default(),
            entity_to_archetype_index: Default::default(),
        }
    }
}

fn create_raw_component_vec<ComponentType: Debug + Send + Sync + 'static>() -> Box<dyn ComponentVec>
{
    Box::new(RwLock::new(Vec::<Option<ComponentType>>::new()))
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

fn intersection_of_multiple_sets<T: Hash + Eq + Clone>(sets: &[HashSet<T>]) -> HashSet<T> {
    let element_overlaps_with_all_other_sets =
        move |element: &&T| sets[1..].iter().all(|set| set.contains(element));
    sets.get(0)
        .unwrap_or(&HashSet::new())
        .iter()
        .filter(element_overlaps_with_all_other_sets)
        .cloned()
        .collect()
}

type ComponentVecImpl<ComponentType> = RwLock<Vec<Option<ComponentType>>>;

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
    /// archetypes component vector of the same data type
    /// If no component vector of matching type exists in target
    /// archetype then this will only perform a swap_remove.
    fn move_element(&self, source_index: usize, target_arch: &Archetype) -> ArchetypeResult<()>;
    /// Adds a component_vec to a new archetype to
    /// prepare for the move function
    fn move_element_to_new(
        &self,
        source_index: usize,
        target_arch: &mut Archetype,
    ) -> ArchetypeResult<()>;
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
    fn move_element(&self, source_index: usize, target_arch: &Archetype) -> ArchetypeResult<()> {
        let value = self
            .write()
            .expect("Lock is poisoned")
            .swap_remove(source_index);

        // Only perform move if component type exists in target_arch.
        if let Some(target_vec) = target_arch
            .component_typeid_to_component_vec
            .get(&TypeId::of::<T>())
        {
            let mut component_vec = borrow_component_vec_mut::<T>(target_vec.deref())
                .ok_or(ArchetypeError::CouldNotBorrowComponentVec(TypeId::of::<T>()))?;

            component_vec.push(value);
        }

        Ok(())
    }

    fn move_element_to_new(
        &self,
        source_index: usize,
        target_arch: &mut Archetype,
    ) -> ArchetypeResult<()> {
        target_arch.add_component_vec::<T>();
        self.move_element(source_index, target_arch)
    }
}

/// An entity is an identifier that represents a simulated object consisting of multiple
/// different components.
#[derive(Default, Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone, Hash)]
pub struct Entity {
    id: usize,
    _generation: usize,
}

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
        // new
        world.add_component_to_entity(entity, A).unwrap();

        //world.create_component_vec_and_add(entity, A).unwrap();

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
        //world.create_component_vec_and_add(entity, A).unwrap();

        let first = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
        drop(first);
        let _second = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
    }

    #[test]
    fn world_does_not_panic_when_trying_to_immutably_borrow_same_components_twice() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap();

        world.add_component_to_entity(entity, A).unwrap();
        //world.create_component_vec_and_add(entity, A).unwrap();

        let _first = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
        let _second = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
    }

    // Archetype tests:

    #[test]
    fn entites_change_archetype_after_component_addition() {
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
    fn moving_last_entity_in_an_archetype() {
        let mut world = World::default();

        let entity = world.create_new_entity().unwrap();

        world.add_component_to_entity(entity, A).unwrap();
        world.add_component_to_entity(entity, B).unwrap();

        let mut actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 2);

        world
            .remove_component_type_from_entity::<A>(entity)
            .unwrap();

        actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 3);

        world
            .remove_component_type_from_entity::<B>(entity)
            .unwrap();

        actual_index = *world.entity_to_archetype_index.get(&entity).unwrap();

        assert_eq!(actual_index, 0);
    }

    #[test]
    fn entity_swapped_in_comp_vec_during_addition_maintains_values() {
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

        let k: &Archetype = world.archetypes.get(2).unwrap();

        let x = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<u32>())
            .unwrap()
            .deref();
        let comp_vec_1 = borrow_component_vec::<u32>(x).unwrap();

        let y = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<i32>())
            .unwrap()
            .deref();
        let comp_vec_2 = borrow_component_vec::<i32>(y).unwrap();

        let value_1_1 = comp_vec_1.get(0).unwrap().unwrap();
        let value_2_1 = comp_vec_1.get(1).unwrap().unwrap();
        let value_3_1 = comp_vec_1.get(2).unwrap().unwrap();
        let value_1_2 = comp_vec_2.get(0).unwrap().unwrap();
        let value_2_2 = comp_vec_2.get(1).unwrap().unwrap();
        let value_3_2 = comp_vec_2.get(2).unwrap().unwrap();

        // Check original values and indexes

        assert_eq!(value_1_1, 1);
        assert_eq!(value_2_1, 2);
        assert_eq!(value_3_1, 3);
        assert_eq!(value_1_2, 1);
        assert_eq!(value_2_2, 2);
        assert_eq!(value_3_2, 3);

        assert_eq!(k.entity_order, vec![entity1, entity2, entity3]);
        assert_eq!(
            k.entity_to_component_index.get(&entity1).unwrap().clone(),
            0
        );
        assert_eq!(
            k.entity_to_component_index.get(&entity2).unwrap().clone(),
            1
        );
        assert_eq!(
            k.entity_to_component_index.get(&entity3).unwrap().clone(),
            2
        );

        drop(comp_vec_1);
        drop(comp_vec_2);

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let k: &Archetype = world.archetypes.get(2).unwrap();

        let x = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<u32>())
            .unwrap()
            .deref();
        let comp_vec_1 = borrow_component_vec::<u32>(x).unwrap();

        let y = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<i32>())
            .unwrap()
            .deref();
        let comp_vec_2 = borrow_component_vec::<i32>(y).unwrap();

        let value_1_1 = comp_vec_1.get(0).unwrap().unwrap();
        let value_2_1 = comp_vec_1.get(1).unwrap().unwrap();
        let value_1_2 = comp_vec_2.get(0).unwrap().unwrap();
        let value_2_2 = comp_vec_2.get(1).unwrap().unwrap();

        // Check new values and indexes

        assert!(comp_vec_1.get(2).is_none());
        assert!(comp_vec_2.get(2).is_none());

        assert_eq!(value_1_1, 3);
        assert_eq!(value_2_1, 2);
        assert_eq!(value_1_2, 3);
        assert_eq!(value_2_2, 2);

        assert_eq!(k.entity_order, vec![entity3, entity2]);
        assert_eq!(k.entity_to_component_index.get(&entity1).clone(), None);
        assert_eq!(
            k.entity_to_component_index.get(&entity2).unwrap().clone(),
            1
        );
        assert_eq!(
            k.entity_to_component_index.get(&entity3).unwrap().clone(),
            0
        );

        drop(comp_vec_1);

        assert_eq!(world.entities, vec![entity1, entity2, entity3]);
        assert_eq!(
            world.entity_to_archetype_index.get(&entity1).unwrap(),
            &3_usize
        );
        assert_eq!(
            world.entity_to_archetype_index.get(&entity2).unwrap(),
            &2_usize
        );
        assert_eq!(
            world.entity_to_archetype_index.get(&entity3).unwrap(),
            &2_usize
        );
    }

    #[test]
    fn entity_swapped_in_comp_vec_during_remove_maintains_values() {
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

        let k: &Archetype = world.archetypes.get(2).unwrap();

        let x = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<u32>())
            .unwrap()
            .deref();
        let comp_vec_1 = borrow_component_vec::<u32>(x).unwrap();

        let y = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<i32>())
            .unwrap()
            .deref();
        let comp_vec_2 = borrow_component_vec::<i32>(y).unwrap();

        let value_1_1 = comp_vec_1.get(0).unwrap().unwrap();
        let value_2_1 = comp_vec_1.get(1).unwrap().unwrap();
        let value_3_1 = comp_vec_1.get(2).unwrap().unwrap();
        let value_1_2 = comp_vec_2.get(0).unwrap().unwrap();
        let value_2_2 = comp_vec_2.get(1).unwrap().unwrap();
        let value_3_2 = comp_vec_2.get(2).unwrap().unwrap();

        // Check original values and indexes

        assert_eq!(value_1_1, 1);
        assert_eq!(value_2_1, 2);
        assert_eq!(value_3_1, 3);
        assert_eq!(value_1_2, 1);
        assert_eq!(value_2_2, 2);
        assert_eq!(value_3_2, 3);

        assert_eq!(k.entity_order, vec![entity1, entity2, entity3]);
        assert_eq!(
            k.entity_to_component_index.get(&entity1).unwrap().clone(),
            0
        );
        assert_eq!(
            k.entity_to_component_index.get(&entity2).unwrap().clone(),
            1
        );
        assert_eq!(
            k.entity_to_component_index.get(&entity3).unwrap().clone(),
            2
        );

        drop(comp_vec_1);
        drop(comp_vec_2);

        // Remove first entities component causing it to move to Arch_1
        world
            .remove_component_type_from_entity::<i32>(entity1)
            .unwrap();

        let k: &Archetype = world.archetypes.get(2).unwrap();

        let x = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<u32>())
            .unwrap()
            .deref();
        let comp_vec_1 = borrow_component_vec::<u32>(x).unwrap();

        let y = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<i32>())
            .unwrap()
            .deref();
        let comp_vec_2 = borrow_component_vec::<i32>(y).unwrap();
        println!("{comp_vec_2:?}");

        let value_1_1 = comp_vec_1.get(0).unwrap().unwrap();
        let value_2_1 = comp_vec_1.get(1).unwrap().unwrap();
        let value_1_2 = comp_vec_2.get(0).unwrap().unwrap();
        let value_2_2 = comp_vec_2.get(1).unwrap().unwrap();

        // Check new values and indexes

        assert!(comp_vec_1.get(2).is_none());
        assert!(comp_vec_2.get(2).is_none());

        assert_eq!(value_1_1, 3);
        assert_eq!(value_2_1, 2);
        assert_eq!(value_1_2, 3);
        assert_eq!(value_2_2, 2);

        assert_eq!(k.entity_order, vec![entity3, entity2]);
        assert_eq!(k.entity_to_component_index.get(&entity1).clone(), None);
        assert_eq!(
            k.entity_to_component_index.get(&entity2).unwrap().clone(),
            1
        );
        assert_eq!(
            k.entity_to_component_index.get(&entity3).unwrap().clone(),
            0
        );

        drop(comp_vec_1);

        assert_eq!(world.entities, vec![entity1, entity2, entity3]);
        assert_eq!(
            world.entity_to_archetype_index.get(&entity1).unwrap(),
            &1_usize
        );
        assert_eq!(
            world.entity_to_archetype_index.get(&entity2).unwrap(),
            &2_usize
        );
        assert_eq!(
            world.entity_to_archetype_index.get(&entity3).unwrap(),
            &2_usize
        );
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
        let k: &Archetype = world.archetypes.get(4).unwrap();

        let x = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<usize>())
            .unwrap()
            .deref();
        let comp_vec_1 = borrow_component_vec::<usize>(x).unwrap();
        let value_1 = comp_vec_1.get(0).unwrap();

        let x = k
            .component_typeid_to_component_vec
            .get(&TypeId::of::<i64>())
            .unwrap()
            .deref();
        let comp_vec_3 = borrow_component_vec::<i64>(x).unwrap();
        let value_3 = comp_vec_3.get(0).unwrap();

        assert_eq!(value_1.unwrap(), 10);
        assert_eq!(value_3.unwrap(), 321);
    }

    fn setup_world_with_three_entities_and_components() -> World {
        // Arrange
        let mut world = World::default();

        let e1 = world.create_new_entity().unwrap();
        let e2 = world.create_new_entity().unwrap();
        let e3 = world.create_new_entity().unwrap();

        // insert some components...
        world.add_component_to_entity::<u64>(e1, 1).unwrap();
        world.add_component_to_entity::<u32>(e1, 2).unwrap();

        world.add_component_to_entity::<u64>(e2, 3).unwrap();
        world.add_component_to_entity::<u32>(e2, 4).unwrap();

        world.add_component_to_entity::<u32>(e3, 6).unwrap();

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
            .flat_map(|component_vec| {
                component_vec
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|component| component.unwrap())
            })
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
            .flat_map(|component_vec| {
                component_vec
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|component| component.unwrap())
            })
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
                if let Some(parameter) = parameter {
                    result.insert(*parameter);
                }
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
        let borrowed_test_vecs: Vec<HashSet<usize>> = test_vecs
            .iter()
            .map(|vec| HashSet::from_iter(vec.clone()))
            .collect();
        let borrowed_expected_value: HashSet<usize> = expected_value.into_iter().collect();

        // Perform intersection operation
        let result = intersection_of_multiple_sets(&borrowed_test_vecs);

        // Assert intersection result equals expected value
        assert_eq!(result, borrowed_expected_value);
    }
}
