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
mod profiling;
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
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};
use thiserror::Error;

/// An error in the application.
#[derive(Error, Debug)]
pub enum BasicApplicationError {
    /// Failed to generate schedule for given systems.
    #[error("failed to generate schedule for given systems")]
    ScheduleGeneration(#[source] ScheduleError),
    /// Failed to execute systems.
    #[error("failed to execute systems")]
    Execution(#[source] ExecutionError),
    /// Failed to execute systems.
    #[error("failed to perform world operation")]
    World(#[source] WorldError),
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

    /// Adds a new component to a given entity.
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> Result<(), Self::Error>;

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
        let entity = self.world.create_new_entity();
        self.world
            .store_entity_in_archetype(entity, 0)
            .map_err(BasicApplicationError::World)?; // todo(#72): Change so that all entities are not stored in the same big archetype, that is change index 0 to be something else.
        Ok(entity)
    }

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
            self.systems.push(Box::new(system.into_system()));
        }
        self
    }

    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> Result<(), Self::Error> {
        self.world
            .create_component_vec_and_add(entity, component)
            .map_err(BasicApplicationError::World)
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
    ///
    #[error("could not find component index of entity: {0:?}")]
    MissingEntityIndex(Entity),

    ///
    #[error("could not borrow component vec of type: {0:?}")]
    CouldNotBorrowComponentVec(TypeId),
}

/// Whether a archetype operation succeeded.
pub type ArchetypeResult<T, E = ArchetypeError> = Result<T, E>;

/// Stores components associated with entity ids.
#[derive(Debug, Default)]
struct Archetype {
    component_typeid_to_component_vec: HashMap<TypeId, Box<dyn ComponentVec>>,
    entity_to_component_index: HashMap<Entity, ComponentIndex>,
    last_entity_added: Entity,
}

impl Archetype {
    /// Adds an `entity_id` to keep track of and store components for.
    ///
    /// The function is idempotent when passing the same `id` multiple times.
    fn store_entity(&mut self, entity: Entity) {
        if !self.entity_to_component_index.contains_key(&entity) {
            let entity_index = self.entity_to_component_index.len();

            self.component_typeid_to_component_vec
                .values_mut()
                .for_each(|component_vec| component_vec.push_none());

            self.entity_to_component_index.insert(entity, entity_index);

            self.last_entity_added = entity;
        }
    }

    // todo(#72): Remove "#[allow(usused)" when removing enitites has been added.
    #[allow(unused)]
    fn remove_entity(&mut self, entity: Entity) -> ArchetypeResult<()> {
        if let Some(&index) = self.entity_to_component_index.get(&entity) {
            self.component_typeid_to_component_vec
                .values()
                .for_each(|vec| vec.remove(index));
            self.entity_to_component_index.remove(&entity);
            // update index of compnonets of entity on last index
            self.entity_to_component_index
                .insert(self.last_entity_added, index);
        } else {
            return Err(ArchetypeError::MissingEntityIndex(entity));
        }
        Ok(())
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

    /// Adds a component of type `ComponentType` to the specified `entity`.
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> ArchetypeResult<()> {
        let entity_index = self
            .entity_to_component_index
            .get(&entity)
            .ok_or(ArchetypeError::MissingEntityIndex(entity))?;

        let mut component_vec = self.borrow_component_vec_mut::<ComponentType>().ok_or(
            ArchetypeError::CouldNotBorrowComponentVec(TypeId::of::<ComponentType>()),
        )?;

        component_vec[*entity_index] = Some(component);
        Ok(())
    }

    /// Adds a component vec of type `ComponentType` if no such vec already exists.
    ///
    /// This function is idempotent when trying to add the same `ComponentType` multiple times.
    fn add_component_vec<ComponentType: Debug + Send + Sync + 'static>(&mut self) {
        if !self.contains_component_type::<ComponentType>() {
            let mut raw_component_vec = create_raw_component_vec::<ComponentType>();

            self.entity_to_component_index
                .iter()
                .for_each(|_| raw_component_vec.push_none());

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
    /// Could not find the given component type.
    #[error("could not find the given component type: {0:?}")]
    ComponentTypeDoesNotExist(TypeId),
    /// Could not find the given entity id.
    #[error("could not find the given entity id: {0:?}")]
    EntityIdDoesNotExist(Entity),
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
    /// todo(#72): Contains all `TypeId`s of the components that `World` stores.
    /// todo(#72): Remove, only used to showcase how archetypes can be used in querying.
    stored_types: Vec<TypeId>,
}

type ReadComponentVec<'a, ComponentType> = Option<RwLockReadGuard<'a, Vec<Option<ComponentType>>>>;
type WriteComponentVec<'a, ComponentType> =
    Option<RwLockWriteGuard<'a, Vec<Option<ComponentType>>>>;

impl World {
    /// todo(#72): Adds the Component to the entity by storing it in the `Big Archetype`.
    /// todo(#72): Adds a new component vec to `Big Archetype` if it does not already exist.
    fn create_component_vec_and_add<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> WorldResult<()> {
        self.store_entity_in_archetype(entity, 0)?;

        let big_archetype = self
            .archetypes
            .get_mut(0)
            .ok_or(WorldError::ArchetypeDoesNotExist(0))?;

        if !big_archetype.contains_component_type::<ComponentType>() {
            let component_typeid = TypeId::of::<ComponentType>();
            self.stored_types.push(component_typeid);
            let archetype_index = 0;
            self.component_typeid_to_archetype_indices
                .entry(component_typeid)
                .or_default()
                .insert(archetype_index);
        }
        // todo(#38) Some code is mistakingly part of Application instead of World

        big_archetype.add_component_vec::<ComponentType>();

        self.add_component(entity, component)?;
        Ok(())
    }

    fn create_new_entity(&mut self) -> Entity {
        let entity_id = self.entities.len();
        let entity = Entity {
            id: entity_id,
            _generation: 0, /* todo(#53) update entity generation after an entity has been removed and then added. */
        };
        self.entities.push(entity);
        entity
    }

    // todo(#72): Remove "#[allow(usused)" when using add empty archetype.
    #[allow(unused)]
    fn add_empty_archetype(&mut self, archetype: Archetype) {
        let archetype_index = self.archetypes.len();

        archetype
            .component_typeid_to_component_vec
            .values()
            .for_each(|component_vec| {
                let component_typeid = component_vec.stored_type();
                self.component_typeid_to_archetype_indices
                    .entry(component_typeid)
                    .or_default()
                    .insert(archetype_index);
            });

        self.archetypes.push(archetype);
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

        // todo(#72): add code for moving entities between archetypes

        self.entity_to_archetype_index
            .insert(entity, archetype_index);
        Ok(())
    }

    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> WorldResult<()> {
        let archetype_index = self
            .entity_to_archetype_index
            .get(&entity)
            .ok_or(WorldError::EntityIdDoesNotExist(entity))?;
        let archetype = self
            .archetypes
            .get_mut(*archetype_index)
            .ok_or(WorldError::ArchetypeDoesNotExist(*archetype_index))?;
        archetype
            .add_component::<ComponentType>(entity, component)
            .map_err(WorldError::CouldNotAddComponent)?;
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
            stored_types: Default::default(),
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
    fn push_none(&mut self);
    /// Returns the type stored in the component vector.
    fn stored_type(&self) -> TypeId;
    /// Returns the number of components stored in the component vector.
    fn len(&self) -> usize;
    /// Removes the entity from the component vector.
    fn remove(&self, index: usize);
}

impl<T: Debug + Send + Sync + 'static> ComponentVec for ComponentVecImpl<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn push_none(&mut self) {
        self.write().expect("Lock is poisoned").push(None);
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

    impl Entity {
        fn with_id(n: usize) -> Self {
            Self {
                id: n,
                _generation: 0,
            }
        }
    }

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

        let entity = world.create_new_entity();
        world.create_component_vec_and_add(entity, A).unwrap();

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

        let entity = world.create_new_entity();

        world.create_component_vec_and_add(entity, A).unwrap();

        let first = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
        drop(first);
        let _second = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
    }

    #[test]
    fn world_does_not_panic_when_trying_to_immutably_borrow_same_components_twice() {
        let mut world = World::default();

        let entity = world.create_new_entity();

        world.create_component_vec_and_add(entity, A).unwrap();

        let _first = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
        let _second = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
    }

    // Archetype tests:

    #[test]
    fn archetype_can_store_components_of_entities_it_stores() {
        let mut archetype = Archetype::default();

        let entity_1 = Entity::with_id(0);
        let entity_2 = Entity::with_id(5);

        // 1. Archetype stores the components of entities with id 0 and id 10
        archetype.store_entity(entity_1);
        archetype.store_entity(entity_2);

        // 2. Create component vectors for types u32 and u64
        archetype.add_component_vec::<u32>();
        archetype.add_component_vec::<u64>();

        // 3. Add components for entity id 0
        archetype.add_component::<u32>(entity_1, 21).unwrap();
        archetype.add_component::<u64>(entity_1, 212).unwrap();

        // 4. Add components for entity id 1
        archetype.add_component::<u32>(entity_2, 35).unwrap();
        archetype.add_component::<u64>(entity_2, 123).unwrap();

        let result_u32 = archetype.borrow_component_vec::<u32>().unwrap();
        let result_u64 = archetype.borrow_component_vec::<u64>().unwrap();

        // entity 1 should have index 0, as it was added first
        assert_eq!(result_u32.get(0).unwrap().unwrap(), 21);
        assert_eq!(result_u64.get(0).unwrap().unwrap(), 212);

        // entity 2 should have index 1 as it was added second
        assert_eq!(result_u32.get(1).unwrap().unwrap(), 35);
        assert_eq!(result_u64.get(1).unwrap().unwrap(), 123);
    }

    #[test]
    fn components_are_stored_on_correct_index_independent_of_insert_order() {
        let mut archetype = Archetype::default();

        let entity_1 = Entity::with_id(0);
        let entity_2 = Entity::with_id(5);
        let entity_3 = Entity::with_id(10);

        let entity_1_index = 0;
        let entity_2_index = 1;
        let entity_3_index = 2;

        // 1. Create component vectors for types u32 and u64
        archetype.add_component_vec::<u32>();
        archetype.add_component_vec::<u64>();

        // 2. store entities
        archetype.store_entity(entity_1);
        archetype.store_entity(entity_2);
        archetype.store_entity(entity_3);

        // 3. add components in random order, with values 1-6
        archetype.add_component::<u32>(entity_2, 1).unwrap();
        archetype.add_component::<u64>(entity_1, 2).unwrap();
        archetype.add_component::<u64>(entity_2, 3).unwrap();
        archetype.add_component::<u32>(entity_3, 4).unwrap();
        archetype.add_component::<u32>(entity_1, 5).unwrap();
        archetype.add_component::<u64>(entity_3, 6).unwrap();

        let result_u32 = archetype.borrow_component_vec::<u32>().unwrap();
        let result_u64 = archetype.borrow_component_vec::<u64>().unwrap();

        // 4. assert values are correct
        assert_eq!(result_u32.get(entity_2_index).unwrap().unwrap(), 1);
        assert_eq!(result_u64.get(entity_1_index).unwrap().unwrap(), 2);
        assert_eq!(result_u64.get(entity_2_index).unwrap().unwrap(), 3);
        assert_eq!(result_u32.get(entity_3_index).unwrap().unwrap(), 4);
        assert_eq!(result_u32.get(entity_1_index).unwrap().unwrap(), 5);
        assert_eq!(result_u64.get(entity_3_index).unwrap().unwrap(), 6);
    }

    #[test]
    fn storing_entities_gives_them_indices_when_component_vec_exists() {
        let mut archetype = Archetype::default();

        let entity_1 = Entity::with_id(81);
        let entity_2 = Entity::with_id(16);
        let entity_3 = Entity::with_id(66);
        let entity_4 = Entity::with_id(140);

        // 1. Add component vec
        archetype.add_component_vec::<u8>();

        // 2. Store 4 entities
        archetype.store_entity(entity_1);
        archetype.store_entity(entity_2);
        archetype.store_entity(entity_3);
        archetype.store_entity(entity_4);

        let result_u8 = archetype.borrow_component_vec::<u8>().unwrap();

        // The component vec should have a length of 4, since that is the number of entities stored
        assert_eq!(result_u8.len(), 4);
        // No values should be stored since none have been added.
        result_u8
            .iter()
            .for_each(|component| assert!(component.is_none()));
    }

    #[test]
    fn adding_component_vec_after_entities_have_been_added_gives_entities_indices_in_the_new_vec() {
        let mut archetype = Archetype::default();

        let entity_1 = Entity::with_id(81);
        let entity_2 = Entity::with_id(16);
        let entity_3 = Entity::with_id(66);
        let entity_4 = Entity::with_id(140);

        // 1. Store 4 entities
        archetype.store_entity(entity_1);
        archetype.store_entity(entity_2);
        archetype.store_entity(entity_3);
        archetype.store_entity(entity_4);

        // 2. Add component vec
        archetype.add_component_vec::<u64>();

        let result_u64 = archetype.borrow_component_vec::<u64>().unwrap();

        // The component vec should have a length of 4, since that is the number of entities stored
        assert_eq!(result_u64.len(), 4);
        // No values should be stored since none have been added.
        result_u64
            .iter()
            .for_each(|compnonent| assert!(compnonent.is_none()));
    }

    #[test]
    fn interleaving_adding_vecs_and_storing_entities_results_in_correct_length_of_vecs() {
        let mut archetype = Archetype::default();

        let entity_1 = Entity::with_id(81);
        let entity_2 = Entity::with_id(16);
        let entity_3 = Entity::with_id(66);
        let entity_4 = Entity::with_id(140);

        // 1. add u8 component vec.
        archetype.add_component_vec::<u8>();

        // 2. store entities 1 and 2.
        archetype.store_entity(entity_1);
        archetype.store_entity(entity_2);

        // 3. add u64 component vec.
        archetype.add_component_vec::<u64>();

        // 4. store entities 3 and 4.
        archetype.store_entity(entity_3);
        archetype.store_entity(entity_4);

        // 5. add f64 component vec.
        archetype.add_component_vec::<f64>();

        let result_u8 = archetype.borrow_component_vec::<u8>().unwrap();
        let result_u64 = archetype.borrow_component_vec::<u64>().unwrap();
        let result_f64 = archetype.borrow_component_vec::<f64>().unwrap();

        // One component for each entity should be stored in each component vec.
        assert_eq!(result_u8.len(), 4);
        assert_eq!(result_u64.len(), 4);
        assert_eq!(result_f64.len(), 4);
    }

    fn setup_world_with_three_entities_and_components() -> World {
        // Arrange
        let mut world = World {
            archetypes: Vec::new(),
            ..Default::default()
        };
        // add archetype index 0
        world.add_empty_archetype({
            let mut archetype = Archetype::default();
            archetype.add_component_vec::<u64>();
            archetype.add_component_vec::<u32>();

            archetype
        });

        // add archetype index 1
        world.add_empty_archetype({
            let mut archetype = Archetype::default();
            archetype.add_component_vec::<u32>();

            archetype
        });

        let e1 = world.create_new_entity();
        let e2 = world.create_new_entity();
        let e3 = world.create_new_entity();

        // e1 and e2 are stored in archetype index 0
        world.store_entity_in_archetype(e1, 0).unwrap();
        world.store_entity_in_archetype(e2, 0).unwrap();
        // e3 is stored in archetype index 1
        world.store_entity_in_archetype(e3, 1).unwrap();

        // insert some components...
        world.add_component::<u64>(e1, 1).unwrap();
        world.add_component::<u32>(e1, 2).unwrap();

        world.add_component::<u64>(e2, 3).unwrap();
        world.add_component::<u32>(e2, 4).unwrap();

        world.add_component::<u32>(e3, 6).unwrap();

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

    #[test]
    fn removing_entity_swaps_position_of_last_added_entitys_components() {
        // Arrange
        let mut archetype = Archetype::default();

        let entity_1 = Entity::with_id(81);
        let entity_2 = Entity::with_id(16);
        let entity_3 = Entity::with_id(66);

        archetype.store_entity(entity_1);
        archetype.store_entity(entity_2);
        archetype.store_entity(entity_3);

        archetype.add_component_vec::<u32>();
        archetype.add_component_vec::<f32>();

        archetype.add_component::<u32>(entity_1, 1).unwrap();
        archetype.add_component::<u32>(entity_2, 2).unwrap();
        archetype.add_component::<u32>(entity_3, 3).unwrap();

        archetype.add_component::<f32>(entity_1, 1.0).unwrap();
        archetype.add_component::<f32>(entity_2, 2.0).unwrap();
        archetype.add_component::<f32>(entity_3, 3.0).unwrap();

        // Act
        archetype.remove_entity(entity_1).unwrap();

        // Assert
        let component_vec_u32 = archetype.borrow_component_vec::<u32>().unwrap();
        let component_vec_f32 = archetype.borrow_component_vec::<f32>().unwrap();

        // Removing entity 1 should move components of entity 3 to index 0.
        assert_eq!(component_vec_u32.get(0).unwrap().unwrap(), 3);
        assert_eq!(component_vec_f32.get(0).unwrap().unwrap(), 3.0);
        // Components of entity 2 should stay on index 1.
        assert_eq!(component_vec_u32.get(1).unwrap().unwrap(), 2);
        assert_eq!(component_vec_f32.get(1).unwrap().unwrap(), 2.0);

        assert_eq!(component_vec_u32.len(), 2);
        assert_eq!(component_vec_f32.len(), 2);
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
