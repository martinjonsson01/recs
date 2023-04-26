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

mod archetypes;
pub mod filter;
pub mod logging;
pub mod profiling;
pub mod systems;

use crate::archetypes::{Archetype, ArchetypeError};
use crate::systems::SystemError::CannotRunSequentially;
use crate::systems::{IntoSystem, System, SystemError, SystemParameters, SystemResult};
use crate::BasicApplicationError::ScheduleGeneration;
use crossbeam::channel::{bounded, Receiver, Sender, TryRecvError};
use fnv::FnvHashMap;
use nohash_hasher::{IsEnabled, NoHashHasher};
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::Debug;
use std::hash::{BuildHasherDefault, Hash};
use std::sync::{RwLockReadGuard, RwLockWriteGuard};
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

fn intersection_of_multiple_sets<T: IsEnabled + Hash + Eq + Clone>(
    sets: &[NoHashHashSet<T>],
) -> NoHashHashSet<T> {
    let element_overlaps_with_all_other_sets =
        move |element: &&T| sets[1..].iter().all(|set| set.contains(element));
    sets.get(0)
        .unwrap_or(&HashSet::default())
        .iter()
        .filter(element_overlaps_with_all_other_sets)
        .cloned()
        .collect()
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
            .remove_component_from_entity::<ComponentType>(entity)
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

type ReadComponentVec<'a, ComponentType> = Option<RwLockReadGuard<'a, Vec<ComponentType>>>;
type WriteComponentVec<'a, ComponentType> = Option<RwLockWriteGuard<'a, Vec<ComponentType>>>;

/// A hashmap where the keys are not hashed
pub(crate) type NoHashHashMap<T, S> = HashMap<T, S, BuildHasherDefault<NoHashHasher<T>>>;

/// A hashset where the keys are not hashed
pub(crate) type NoHashHashSet<T> = HashSet<T, BuildHasherDefault<NoHashHasher<T>>>;

/// Represents the simulated world.
#[derive(Debug)]
pub struct World {
    entities: Vec<Entity>,
    /// Relates a unique `Entity Id` to the `Archetype` that stores it.
    /// The HashMap returns the corresponding `index` of the `Archetype` stored in the `World.archetypes` vector.
    entity_to_archetype_index: NoHashHashMap<Entity, ArchetypeIndex>,
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
    component_typeid_to_archetype_indices: FnvHashMap<TypeId, NoHashHashSet<ArchetypeIndex>>,

    /// `component_typeids_set_to_archetype_indices` is a HashMap relating a set of Component
    /// `TypeId`s to the unique `Archetype` that stores them.
    /// Its purpose it to allow for fast retrieval of an `Archetype` when adding or removing a
    /// `Component` from an `Entity`
    ///
    /// Since the key is a `Vec<TypeId>` we need to sort when inserting key-pair values and fetching
    /// from a key. Most uses consists of retrieving the `TypeId`s from a hashmap iterator which
    /// is not guaranteed to return the keys in any specific order.
    component_typeids_set_to_archetype_index: FnvHashMap<Vec<TypeId>, ArchetypeIndex>,
}

impl World {
    fn create_new_entity(&mut self) -> WorldResult<Entity> {
        self.create_entity_in_empty_archetype()
    }

    fn add_component_to_entity<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> WorldResult<()> {
        self.add_component_to_entity_and_move_archetype(entity, component)
    }

    fn remove_component_from_entity<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
    ) -> WorldResult<()> {
        self.remove_component_type_from_entity::<ComponentType>(entity)
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

    /// Returns the indices of all archetypes that at least contain the given signature.
    ///
    /// An example: if there exists the archetypes: (A), (A,B), (B,C), (A,B,C)
    /// and the signature (A,B) is given, the indices for archetypes: (A,B) and
    /// (A,B,C) will be returned as they both contain (A,B), while (A) only
    /// contains A components and no B components and (B,C) only contain B and C
    /// components and no A components.
    fn get_archetype_indices(&self, signature: &[TypeId]) -> NoHashHashSet<ArchetypeIndex> {
        let all_archetypes_with_signature_types: WorldResult<Vec<NoHashHashSet<ArchetypeIndex>>> =
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
            Err(_) => HashSet::default(),
        }
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

/// An entity is an identifier that represents a simulated object consisting of multiple
/// different components.
#[derive(Default, Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
pub struct Entity {
    id: usize,
}

impl Hash for Entity {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        hasher.write_usize(self.id)
    }
}

impl IsEnabled for Entity {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::{Read, SystemParameter};
    use test_case::test_case;
    use test_log::test;

    #[derive(Debug)]
    struct A;

    #[derive(Debug)]
    struct B;

    #[derive(Debug)]
    struct C;

    #[test]
    fn querying_with_archetypes() {
        // Arrange
        let (world, _, _, _, _) = setup_world_with_3_entities_with_u32_and_i32_components();

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

        assert_eq!(result, HashSet::from([1, 2, 3]))
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
    ) -> (World, ArchetypeIndex, Entity, Entity, Entity) {
        let mut world = World::default();

        let entity1 = world.create_new_entity().unwrap();
        let entity2 = world.create_new_entity().unwrap();
        let entity3 = world.create_new_entity().unwrap();

        world.add_component_to_entity::<u32>(entity1, 1).unwrap();
        world.add_component_to_entity::<i32>(entity1, 1).unwrap();

        world.add_component_to_entity::<u32>(entity2, 2).unwrap();
        world.add_component_to_entity::<i32>(entity2, 2).unwrap();

        world.add_component_to_entity::<u32>(entity3, 3).unwrap();
        world.add_component_to_entity::<i32>(entity3, 3).unwrap();

        // All entities in archetype with index 2 now
        (world, 2, entity1, entity2, entity3)
    }

    #[test]
    fn entities_contain_correct_values_after_adding_components() {
        let (world, relevant_archetype_index, _, _, _) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let u32_values = archetype.borrow_component_vec::<u32>().unwrap();
        let i32_values = archetype.borrow_component_vec::<i32>().unwrap();

        assert_eq!(&[1_u32, 2_u32, 3_u32], &u32_values[..]);
        assert_eq!(&[1_i32, 2_i32, 3_i32], &i32_values[..]);
    }

    #[test]
    fn entity_values_are_removed_from_archetype_after_addition() {
        let (mut world, relevant_archetype_index, entity1, _, _) =
            setup_world_with_3_entities_with_u32_and_i32_components();

        // Add component to entity1 causing it to move to Arch_3
        world.add_component_to_entity(entity1, 1_usize).unwrap();

        let archetype = world.archetypes.get(relevant_archetype_index).unwrap();

        let u32_values = archetype.borrow_component_vec::<u32>().unwrap();
        let i32_values = archetype.borrow_component_vec::<i32>().unwrap();

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

        let u32_values = archetype.borrow_component_vec::<u32>().unwrap();
        let i32_values = archetype.borrow_component_vec::<i32>().unwrap();

        assert_eq!(&u32_values[..2], &[3_u32, 2_u32]);
        assert_eq!(&i32_values[..2], &[3_i32, 2_i32]);
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

        let arch_2_u32_values = archetype_2.borrow_component_vec::<u32>().unwrap();
        let arch_2_i32_values = archetype_2.borrow_component_vec::<i32>().unwrap();

        assert_eq!([3_u32, 2_u32], arch_2_u32_values[..]);
        assert_eq!([3_i32, 2_i32], arch_2_i32_values[..]);
        assert_eq!([1_i32], arch_3_i32_values[..]);
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

        let u32_read_vec = archetype.borrow_component_vec::<u32>().unwrap();
        let i32_read_vec = archetype.borrow_component_vec::<i32>().unwrap();
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

        let u32_values = archetype.borrow_component_vec::<u32>().unwrap();
        let i32_values = archetype.borrow_component_vec::<i32>().unwrap();

        assert_eq!(&u32_values[..2], &[3_u32, 2_u32]);
        assert_eq!(&i32_values[..2], &[3_i32, 2_i32]);
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
        let borrowed_test_vecs: Vec<NoHashHashSet<usize>> = test_vecs
            .iter()
            .map(|vec| HashSet::from_iter(vec.clone()))
            .collect();
        let borrowed_expected_value: NoHashHashSet<usize> = expected_value.into_iter().collect();

        // Perform intersection operation
        let result = intersection_of_multiple_sets(&borrowed_test_vecs);

        // Assert intersection result equals expected value
        assert_eq!(result, borrowed_expected_value);
    }
}
