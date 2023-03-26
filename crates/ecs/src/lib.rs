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

pub mod logging;
mod profiling;

use crate::ApplicationError::ScheduleGeneration;
use core::panic;
use crossbeam::channel::{bounded, Receiver, Sender, TryRecvError};
use paste::paste;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};
use std::{any, fmt};
use thiserror::Error;
use tracing::instrument;

/// An error in the application.
#[derive(Error, Debug)]
pub enum ApplicationError {
    /// Failed to generate schedule for given systems.
    #[error("failed to generate schedule for given systems")]
    ScheduleGeneration(#[source] ScheduleError),
    /// Failed to execute systems.
    #[error("failed to execute systems")]
    Execution(#[source] ExecutionError),
}

/// Whether an operation on the application succeeded.
pub type AppResult<T, E = ApplicationError> = Result<T, E>;

/// The entry-point of the entire program, containing all of the entities, components and systems.
#[derive(Default, Debug)]
pub struct Application {
    world: World,
    systems: Vec<Box<dyn System>>,
}

impl Application {
    // TODO: Remove. Temporary code for working with archetypes as if they were the Good ol' ComponentVecs implementation.
    /// Returns default values of Application with World containing single Archetype.
    pub fn new() -> Self {
        Self {
            world: World::new(),
            ..Default::default()
        }
    }

    /// Spawns a new entity in the world.
    pub fn create_entity(&mut self) -> Entity {
        let entity = self.world.create_new_entity();
        self.world.store_entity_in_archetype(entity.id, 0); // Temporary: All entities are stored in the same archetype
        entity
    }

    /// Registers a new system to run in the world.
    pub fn add_system<System, Parameters>(mut self, system: System) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.systems.push(Box::new(system.into_system()));
        self
    }

    /// Registers multiple new systems to run in the world.
    pub fn add_systems<System, Parameters>(
        mut self,
        systems: impl IntoIterator<Item = System>,
    ) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        for system in systems {
            self.systems.push(Box::new(system.into_system()));
        }
        self
    }

    /// Adds a new component to a given entity.
    pub fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        self.world.create_component_vec_and_add(entity, component);
        // self.world.add_component(entity.id, component);
    }

    /// Starts the application. This function does not return until the shutdown command has
    /// been received.
    pub fn run<'systems, E: Executor<'systems>, S: Schedule<'systems>>(
        &'systems mut self,
        shutdown_receiver: Receiver<()>,
    ) -> AppResult<()> {
        let schedule = S::generate(&self.systems).map_err(ScheduleGeneration)?;
        let mut executor = E::default();
        executor
            .execute(schedule, &self.world, shutdown_receiver)
            .map_err(ApplicationError::Execution)
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
                batch.run(world);
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
    pub fn run(&self, world: &World) {
        self.system.run(world);
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

/// Stores components associated with entity ids.
#[derive(Debug, Default)]
struct Archetype {
    component_typeid_to_component_vec: HashMap<TypeId, Box<dyn ComponentVec>>,
    entity_id_to_component_index: HashMap<usize, usize>,
    last_entity_id_added: usize,
}

impl Archetype {
    /// Adds an `entity_id` to keep track of and store components for.
    fn store_entity(&mut self, entity_id: usize) {
        let entity_index = self.entity_id_to_component_index.len();

        self.component_typeid_to_component_vec
            .values_mut()
            .for_each(|v| v.push_none());

        self.entity_id_to_component_index
            .insert(entity_id, entity_index);
        self.last_entity_id_added = entity_id;
    }

    #[allow(unused)]
    fn remove_entity(&mut self, entity_id: usize) {
        if let Some(&index) = self.entity_id_to_component_index.get(&entity_id) {
            self.component_typeid_to_component_vec
                .values()
                .for_each(|vec| vec.swap_remove(index));
            self.entity_id_to_component_index.remove(&entity_id);
            // update index of compnonets of entity on last index
            self.entity_id_to_component_index
                .insert(self.last_entity_id_added, index);
        }
    }

    /// Returns a `ReadComponentVec` with the specified generic type `ComponentType` if it is stored.
    fn borrow_component_vec<ComponentType: Debug + Send + Sync + 'static>(
        &self,
    ) -> ReadComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        if let Some(component_vec) = self
            .component_typeid_to_component_vec
            .get(&component_typeid)
        {
            return borrow_component_vec(component_vec.as_ref());
        }
        None
    }

    /// Returns a `WriteComponentVec` with the specified generic type `ComponentType` if it is stored.
    fn borrow_component_vec_mut<ComponentType: Debug + Send + Sync + 'static>(
        &self,
    ) -> WriteComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        if let Some(component_vec) = self
            .component_typeid_to_component_vec
            .get(&component_typeid)
        {
            return borrow_component_vec_mut(component_vec.as_ref());
        }
        None
    }

    /// Adds a component of type `ComponentType` to the specified `entity`.
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity_id: usize,
        component: ComponentType,
    ) {
        if let Some(&entity_index) = self.entity_id_to_component_index.get(&entity_id) {
            if let Some(mut component_vec) = self.borrow_component_vec_mut::<ComponentType>() {
                component_vec[entity_index] = Some(component);
            }
        }
    }

    /// Adds a component vec of type `ComponentType` if no such vec already exists.
    fn add_component_vec<ComponentType: Debug + Send + Sync + 'static>(&mut self) {
        if !self.contains::<ComponentType>() {
            let mut raw_component_vec = create_raw_component_vec::<ComponentType>();

            for _ in 0..self.entity_id_to_component_index.len() {
                raw_component_vec.push_none();
            }

            let component_typeid = TypeId::of::<ComponentType>();
            self.component_typeid_to_component_vec
                .insert(component_typeid, raw_component_vec);
        }
    }

    /// Returns `true` if the archetype stores components of type ComponentType.
    fn contains<ComponentType: Debug + Send + Sync + 'static>(&self) -> bool {
        self.component_typeid_to_component_vec
            .contains_key(&TypeId::of::<ComponentType>())
    }
}

/// Represents the simulated world.
#[derive(Default, Debug)]
pub struct World {
    entities: Vec<Entity>,
    entity_id_to_archetype_index: HashMap<usize, usize>,
    archetypes: Vec<Archetype>,
    component_typeid_to_archetype_indices: HashMap<TypeId, Vec<usize>>,
    stored_types: Vec<TypeId>, /* TODO: Remove. Used to showcase how archetypes can be used in querying. */
}

type ReadComponentVec<'a, ComponentType> = Option<RwLockReadGuard<'a, Vec<Option<ComponentType>>>>;
type WriteComponentVec<'a, ComponentType> =
    Option<RwLockWriteGuard<'a, Vec<Option<ComponentType>>>>;

impl World {
    // TODO: Remove. Temporary code for working with archetypes as if they were the Good ol' ComponentVecs implementation.
    /// Returns World with single "Big Archetype", corresponding to the Good ol' ComponentVecs
    pub fn new() -> Self {
        Self {
            archetypes: vec![Archetype::default()],
            ..Default::default()
        }
    }

    /// Adds the Component to the entity by storing it in the `Big Archetype`.
    /// Adds a new component vec to `Big Archetype` if it does not already exist.
    fn create_component_vec_and_add<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        let big_archetype = match self.archetypes.get_mut(0) {
            Some(big_archetype) => big_archetype,
            None => {
                self.add_empty_archetype(Archetype::default());
                self.archetypes.get_mut(0).expect("just added")
            }
        };

        if !big_archetype.contains::<ComponentType>() {
            let component_typeid = TypeId::of::<ComponentType>();
            self.stored_types.push(component_typeid);
            // ↓↓↓↓ copied code from fn add_empty_archetype(...) ↓↓↓↓
            let archetype_index = 0;
            match self
                .component_typeid_to_archetype_indices
                .get_mut(&component_typeid)
            {
                Some(indices) => indices.push(archetype_index),
                None => {
                    self.component_typeid_to_archetype_indices
                        .insert(component_typeid, vec![archetype_index]);
                }
            }
            // ↑↑↑↑ copied code from fn add_empty_archetype(...) ↑↑↑↑
        }

        big_archetype.add_component_vec::<ComponentType>();

        self.add_component(entity.id, component);
    }

    fn create_new_entity(&mut self) -> Entity {
        let entity_id = self.entities.len();
        let entity = Entity {
            id: entity_id,
            _generation: 0,
        };
        self.entities.push(entity);
        entity
    }

    fn add_empty_archetype(&mut self, archetype: Archetype) {
        let archetype_index = self.archetypes.len();

        archetype
            .component_typeid_to_component_vec
            .values()
            .for_each(|v| {
                let component_typeid = v.stored_type();
                match self
                    .component_typeid_to_archetype_indices
                    .get_mut(&component_typeid)
                {
                    Some(indices) => indices.push(archetype_index),
                    None => {
                        self.component_typeid_to_archetype_indices
                            .insert(component_typeid, vec![archetype_index]);
                    }
                }
            });

        self.archetypes.push(archetype);
    }

    fn store_entity_in_archetype(&mut self, entity_id: usize, archetype_index: usize) {
        let archetype = self
            .archetypes
            .get_mut(archetype_index)
            .expect("Archetype does not exist");

        archetype.store_entity(entity_id);

        if self.entity_id_to_archetype_index.contains_key(&entity_id) {
            todo!("Add code for cleaning up old archetype");
        }

        self.entity_id_to_archetype_index
            .insert(entity_id, archetype_index);
    }

    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity_id: usize,
        component: ComponentType,
    ) {
        if let Some(&archetype_index) = self.entity_id_to_archetype_index.get(&entity_id) {
            let archetype = self
                .archetypes
                .get_mut(archetype_index)
                .expect("Archetype is missing");
            archetype.add_component::<ComponentType>(entity_id, component);
        }
    }

    fn borrow_component_vecs_with_signature<ComponentType: Debug + Send + Sync + 'static>(
        &self,
        signature: &[TypeId],
    ) -> Vec<ReadComponentVec<ComponentType>> {
        let archetype_indices = self.get_archetype_indices(signature);
        self.borrow_component_vecs(&archetype_indices)
    }

    fn borrow_component_vecs_with_signature_mut<ComponentType: Debug + Send + Sync + 'static>(
        &self,
        signature: &[TypeId],
    ) -> Vec<WriteComponentVec<ComponentType>> {
        let archetype_indices = self.get_archetype_indices(signature);
        self.borrow_component_vecs_mut(&archetype_indices)
    }

    fn get_archetype_indices(&self, signature: &[TypeId]) -> Vec<&usize> {
        // Selects all archetypes that contain the types specified in signature.
        // Ex. if the signature is (A,B,C) then we will find the indices of
        // archetypes: (A), (A,B), (C), (A,B,C,D), because they all contain
        // some of the types from the signature.
        let all_archetypes_with_signature_types: Vec<&Vec<usize>> = signature
            .iter()
            .map(|x| {
                self.component_typeid_to_archetype_indices
                    .get(x)
                    .expect("Archetype does not exist")
            })
            .collect();

        // Select only the archetypes that contain all of the types in signature.
        // Ex. continuing with the example above, where the signature is (A,B,C)
        // only the archetype (A,B,C,D) will be returned.
        intersection(all_archetypes_with_signature_types)
    }

    fn borrow_component_vecs<ComponentType: Debug + Send + Sync + 'static>(
        &self,
        archetype_indices: &[&usize],
    ) -> Vec<ReadComponentVec<ComponentType>> {
        archetype_indices
            .iter()
            .map(|&&archetype_index| {
                self.archetypes
                    .get(archetype_index)
                    .expect("Archetype does not exist")
                    .borrow_component_vec()
            })
            .collect()
    }

    fn borrow_component_vecs_mut<ComponentType: Debug + Send + Sync + 'static>(
        &self,
        archetype_indices: &[&usize],
    ) -> Vec<WriteComponentVec<ComponentType>> {
        archetype_indices
            .iter()
            .map(|&&archetype_index| {
                self.archetypes
                    .get(archetype_index)
                    .expect("Archetype does not exist")
                    .borrow_component_vec_mut()
            })
            .collect()
    }
}

fn panic_locked_component_vec<ComponentType: 'static>() -> ! {
    let component_type_name = any::type_name::<ComponentType>();
    panic!(
        "Lock of ComponentVec<{}> is already taken!",
        component_type_name
    )
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

fn intersection(vecs: Vec<&Vec<usize>>) -> Vec<&usize> {
    let (head, tail) = vecs.split_at(1);
    let head = &head[0];
    head.iter()
        .filter(|x| tail.iter().all(|v| v.contains(x)))
        .collect()
}

type ComponentVecImpl<ComponentType> = RwLock<Vec<Option<ComponentType>>>;

trait ComponentVec: Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn push_none(&mut self);
    fn stored_type(&self) -> TypeId;
    fn len(&self) -> usize;
    fn swap_remove(&self, index: usize);
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

    /// Returns the type stored in the component vector.
    fn stored_type(&self) -> TypeId {
        TypeId::of::<T>()
    }

    /// Returns the number of components stored in the component vector.
    fn len(&self) -> usize {
        Vec::len(&self.read().expect("Lock is poisoned"))
    }

    fn swap_remove(&self, index: usize) {
        self.write().expect("Lock is poisoned").swap_remove(index);
    }
}

/// An entity is an identifier that represents a simulated object consisting of multiple
/// different components.
#[derive(Default, Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
pub struct Entity {
    id: usize,
    _generation: usize,
}

/// An executable unit of work that may operate on entities and their component data.
pub trait System: Debug + Send + Sync {
    /// What the system is called.
    fn name(&self) -> &str;
    /// Executes the system on each entity matching its query.
    ///
    /// Systems that do not query anything run once per tick.
    fn run(&self, world: &World);
    /// Which component types the system accesses and in what manner (read/write).
    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor>;
}

impl Display for dyn System + '_ {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl PartialEq<Self> for dyn System + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name() && self.component_accesses() == other.component_accesses()
    }
}

impl Eq for dyn System + '_ {}

impl Hash for dyn System + '_ {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name().hash(state);
        self.component_accesses().hash(state);
    }
}

/// What component is accessed and in what manner (read/write).
#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum ComponentAccessDescriptor {
    /// Reads from component of provided type.
    Read(TypeId, String),
    /// Reads and writes from component of provided type.
    Write(TypeId, String),
}

impl ComponentAccessDescriptor {
    /// Creates a [`ComponentAccessDescriptor`] that represents
    /// a read of the given [`ComponentType`].
    fn read<ComponentType: 'static>() -> Self {
        let type_id = TypeId::of::<ComponentType>();
        let type_name = any::type_name::<ComponentType>();
        Self::Read(type_id, type_name.to_owned())
    }

    /// Creates a [`ComponentAccessDescriptor`] that represents
    /// a read or write of the given [`ComponentType`].
    fn write<ComponentType: 'static>() -> Self {
        let type_id = TypeId::of::<ComponentType>();
        let type_name = any::type_name::<ComponentType>();
        Self::Write(type_id, type_name.to_owned())
    }

    /// The name of the type of component accessed.
    pub fn name(&self) -> &str {
        match self {
            ComponentAccessDescriptor::Read(_, name)
            | ComponentAccessDescriptor::Write(_, name) => name,
        }
    }

    /// Gets the inner component type.
    pub fn component_type(&self) -> TypeId {
        match &self {
            ComponentAccessDescriptor::Read(component_type, _)
            | ComponentAccessDescriptor::Write(component_type, _) => *component_type,
        }
    }

    /// Whether the access is mutable (read/write).
    pub fn is_write(&self) -> bool {
        match &self {
            ComponentAccessDescriptor::Read(_, _) => false,
            ComponentAccessDescriptor::Write(_, _) => true,
        }
    }

    /// Whether the access is immutable (read).
    pub fn is_read(&self) -> bool {
        !self.is_write()
    }
}

/// Something that can be turned into a `ecs::System`.
pub trait IntoSystem<Parameters> {
    /// What type of system is created.
    type Output: System + 'static;

    /// Turns `self` into an `ecs::System`.
    fn into_system(self) -> Self::Output;
}

/// A `ecs::System` represented by a Rust function/closure.
pub struct FunctionSystem<Function: Send + Sync, Parameters: SystemParameters> {
    function: Function,
    function_name: String,
    parameters: PhantomData<Parameters>,
}

impl<Function: Send + Sync, Parameters: SystemParameters> Debug
    for FunctionSystem<Function, Parameters>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let parameters_name = any::type_name::<Parameters>();
        let mut parameter_names_text = String::with_capacity(parameters_name.len());
        for parameter_name in parameters_name.split(',') {
            parameter_names_text.push_str(parameter_name);
        }

        f.debug_struct("FunctionSystem")
            .field("system", &self.function_name)
            .field("parameters", &parameter_names_text)
            .finish()
    }
}

impl<Function, Parameters> System for FunctionSystem<Function, Parameters>
where
    Function: SystemParameterFunction<Parameters> + Send + Sync + 'static,
    Parameters: SystemParameters,
{
    fn name(&self) -> &str {
        &self.function_name
    }

    #[instrument(skip_all)]
    fn run(&self, world: &World) {
        SystemParameterFunction::run(&self.function, world);
    }

    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor> {
        Parameters::component_accesses()
    }
}

impl<Function, Parameters> IntoSystem<Parameters> for Function
where
    Function: SystemParameterFunction<Parameters> + Send + Sync + 'static,
    Parameters: SystemParameters + 'static,
{
    type Output = FunctionSystem<Function, Parameters>;

    fn into_system(self) -> Self::Output {
        let function_name = get_function_name::<Function>();
        FunctionSystem {
            function: self,
            function_name,
            parameters: PhantomData,
        }
    }
}

fn get_function_name<Function>() -> String {
    let function_type_name = any::type_name::<Function>();
    let name_start_index = function_type_name
        .rfind("::")
        .map_or(0, |colon_index| colon_index + 2);
    let function_name = &function_type_name[name_start_index..];
    function_name.to_owned()
}

/// A collection of `ecs::SystemParameter`s that can be passed to a `ecs::System`.
pub trait SystemParameters: Send + Sync {
    /// A description of all of the data that is accessed and how (read/write).
    fn component_accesses() -> Vec<ComponentAccessDescriptor>;
}

/// Something that can be passed to a `ecs::System`.
pub trait SystemParameter: Send + Sync + Sized {
    /// Contains a borrow of components from `ecs::World`.
    type BorrowedData<'components>;

    /// Borrows the collection of components of the given type from `ecs::World`.
    fn borrow<'a>(world: &'a World, signature: &[TypeId]) -> Self::BorrowedData<'a>;

    /// Fetches the parameter from the borrowed data for a given entity.
    /// # Safety
    /// The returned value is only guaranteed to be valid until BorrowedData is dropped
    unsafe fn fetch_parameter(borrowed: &mut Self::BorrowedData<'_>) -> Option<Option<Self>>;

    /// A description of what data is accessed and how (read/write).
    fn component_access() -> ComponentAccessDescriptor;

    /// Returns the `TypeId` of the borrowed data.
    fn signature() -> TypeId {
        match Self::component_access() {
            ComponentAccessDescriptor::Read(type_id, _)
            | ComponentAccessDescriptor::Write(type_id, _) => type_id,
        }
    }
}

trait SystemParameterFunction<Parameters: SystemParameters>: 'static {
    fn run(&self, world: &World);
}

/// A read-only access to a component of the given type.
#[derive(Debug)]
pub struct Read<'a, Component: 'static> {
    output: &'a Component,
}

impl<'a, Component> Deref for Read<'a, Component> {
    type Target = Component;

    fn deref(&self) -> &Self::Target {
        self.output
    }
}

#[inline(always)]
fn get_and_inc(value: &mut usize) -> usize {
    let tmp = *value;
    *value += 1;
    tmp
}

impl<Component: Debug + Send + Sync + 'static + Sized> SystemParameter for Read<'_, Component> {
    type BorrowedData<'components> = (
        usize,
        Vec<(usize, ReadComponentVec<'components, Component>)>,
    );

    fn borrow<'a>(world: &'a World, signature: &[TypeId]) -> Self::BorrowedData<'a> {
        (
            0,
            world
                .borrow_component_vecs_with_signature::<Component>(signature)
                .into_iter()
                .map(|component_vec| (0, component_vec))
                .collect(),
        )
    }

    unsafe fn fetch_parameter(borrowed: &mut Self::BorrowedData<'_>) -> Option<Option<Self>> {
        if let Some(archetype) = (borrowed.1).get_mut(borrowed.0) {
            if let Some(component_vec) = &archetype.1 {
                return if let Some(component) = component_vec.get(get_and_inc(&mut archetype.0)) {
                    if let Some(component) = component {
                        return Some(Some(Self {
                            // The caller is responsible to only use the
                            // returned value when BorrowedData is still in scope.
                            #[allow(trivial_casts)]
                            output: &*(component as *const Component),
                        }));
                    }
                    Some(None)
                } else {
                    // End of archetype
                    borrowed.0 += 1;
                    Self::fetch_parameter(borrowed)
                };
            }
        }
        // No more entities
        None
    }

    fn component_access() -> ComponentAccessDescriptor {
        ComponentAccessDescriptor::read::<Component>()
    }
}

impl<Component: Debug + Send + Sync + 'static + Sized> SystemParameter for Write<'_, Component> {
    type BorrowedData<'components> = (
        usize,
        Vec<(usize, WriteComponentVec<'components, Component>)>,
    );

    fn borrow<'a>(world: &'a World, signature: &[TypeId]) -> Self::BorrowedData<'a> {
        (
            0,
            world
                .borrow_component_vecs_with_signature_mut::<Component>(signature)
                .into_iter()
                .map(|component_vec| (0, component_vec))
                .collect(),
        )
    }

    unsafe fn fetch_parameter(borrowed: &mut Self::BorrowedData<'_>) -> Option<Option<Self>> {
        if let Some(archetype) = (borrowed.1).get_mut(borrowed.0) {
            if let Some(ref mut component_vec) = &mut archetype.1 {
                return if let Some(component) = component_vec.get_mut(get_and_inc(&mut archetype.0))
                {
                    if let Some(ref mut component) = component {
                        return Some(Some(Self {
                            // The caller is responsible to only use the
                            // returned value when BorrowedData is still in scope.
                            #[allow(trivial_casts)]
                            output: &mut *(component as *mut Component),
                        }));
                    }
                    Some(None)
                } else {
                    // End of archetype
                    borrowed.0 += 1;
                    Self::fetch_parameter(borrowed)
                };
            }
        }
        // No more entities
        None
    }

    fn component_access() -> ComponentAccessDescriptor {
        ComponentAccessDescriptor::write::<Component>()
    }
}

/// A read-only access to a component of the given type.
#[derive(Debug)]
pub struct Write<'a, Component: 'static> {
    output: &'a mut Component,
}

impl<'a, Component> Deref for Write<'a, Component> {
    type Target = Component;

    fn deref(&self) -> &Self::Target {
        self.output
    }
}

impl<'a, Component> DerefMut for Write<'a, Component> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.output
    }
}

impl SystemParameters for () {
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![]
    }
}

impl<F> SystemParameterFunction<()> for F
where
    F: Fn() + 'static,
{
    fn run(&self, _world: &World) {
        self();
    }
}

macro_rules! impl_system_parameter_function {
    ($($parameter:expr),*) => {
        paste! {
            impl<$([<P$parameter>]: SystemParameter,)*> SystemParameters for ($([<P$parameter>],)*) {
                fn component_accesses() -> Vec<ComponentAccessDescriptor> {
                    vec![$([<P$parameter>]::component_access(),)*]
                }
            }

            impl<F, $([<P$parameter>]: SystemParameter,)*> SystemParameterFunction<($([<P$parameter>],)*)>
                for F where F: Fn($([<P$parameter>],)*) + 'static, {

                fn run(&self, world: &World) {
                    let signature = vec![$(<[<P$parameter>] as SystemParameter>::signature(),)*];

                    $(let mut [<borrowed_$parameter>] = <[<P$parameter>] as SystemParameter>::borrow(world, &signature);)*

                    // SAFETY: This is safe because the result from fetch_parameter will not outlive borrowed
                    unsafe {
                        while let ($(Some([<parameter_$parameter>]),)*) = (
                            $(<[<P$parameter>] as SystemParameter>::fetch_parameter(&mut [<borrowed_$parameter>]),)*
                        ) {
                            if let ($(Some([<parameter_$parameter>]),)*) = (
                                $([<parameter_$parameter>],)*
                            ) {
                                self($([<parameter_$parameter>],)*);
                            }
                        }
                    }
                }
            }
        }
    }
}

impl_system_parameter_function!(0);
impl_system_parameter_function!(0, 1);
impl_system_parameter_function!(0, 1, 2);
impl_system_parameter_function!(0, 1, 2, 3);
impl_system_parameter_function!(0, 1, 2, 3, 4);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14);
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;
    use test_log::test;

    #[derive(Debug)]
    struct A;

    #[derive(Debug)]
    struct B;

    #[derive(Debug)]
    struct C;

    #[test]
    #[should_panic(expected = "Lock of ComponentVec<ecs::tests::A> is already taken!")]
    fn world_panics_when_trying_to_mutably_borrow_same_components_twice() {
        let mut world = World::new();

        let entity = Entity {
            id: 0,
            _generation: 1,
        };
        world.entities.push(entity);

        world.create_component_vec_and_add(entity, A);

        let _first = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
        let _second = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
    }

    #[test]
    fn world_doesnt_panic_when_mutably_borrowing_components_after_dropping_previous_mutable_borrow()
    {
        let mut world = World::new();

        let entity = Entity {
            id: 0,
            _generation: 1,
        };
        world.entities.push(entity);

        world.create_component_vec_and_add(entity, A);

        let first = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
        drop(first);
        let _second = world.borrow_component_vecs_with_signature_mut::<A>(&[TypeId::of::<A>()]);
    }

    #[test]
    fn world_does_not_panic_when_trying_to_immutably_borrow_same_components_twice() {
        let mut world = World::new();

        let entity = Entity {
            id: 0,
            _generation: 1,
        };
        world.entities.push(entity);

        world.create_component_vec_and_add(entity, A);

        let _first = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
        let _second = world.borrow_component_vecs_with_signature::<A>(&[TypeId::of::<A>()]);
    }

    #[test_case(|_: Read<A>| {}, vec![ComponentAccessDescriptor::read::<A>()]; "when reading")]
    #[test_case(|_: Write<A>| {}, vec![ComponentAccessDescriptor::write::<A>()]; "when writing")]
    #[test_case(|_: Read<A>, _:Read<B>| {}, vec![ComponentAccessDescriptor::read::<A>(), ComponentAccessDescriptor::read::<B>()]; "when reading two components")]
    #[test_case(|_: Write<A>, _:Write<B>| {}, vec![ComponentAccessDescriptor::write::<A>(), ComponentAccessDescriptor::write::<B>()]; "when writing two components")]
    #[test_case(|_: Read<A>, _: Write<B>| {}, vec![ComponentAccessDescriptor::read::<A>(), ComponentAccessDescriptor::write::<B>()]; "when reading and writing to components")]
    #[test_case(|_: Read<A>, _: Read<B>, _: Read<C>| {}, vec![ComponentAccessDescriptor::read::<A>(), ComponentAccessDescriptor::read::<B>(), ComponentAccessDescriptor::read::<C>()]; "when reading three components")]
    fn component_accesses_return_actual_component_accesses<Params>(
        system: impl IntoSystem<Params>,
        expected_accesses: Vec<ComponentAccessDescriptor>,
    ) {
        let component_accesses = system.into_system().component_accesses();

        assert_eq!(expected_accesses, component_accesses)
    }

    // Archetype tests:

    #[test]
    fn archetype_can_store_components_of_entities_it_stores() {
        let mut archetype = Archetype::default();

        let entity_1_id = 0;
        let entity_2_id = 10;

        // 1. Archetype stores the components of entities with id 0 and id 10
        archetype.store_entity(entity_1_id);
        archetype.store_entity(entity_2_id);

        // 2. Create component vectors for types u32 and u64
        archetype.add_component_vec::<u32>();
        archetype.add_component_vec::<u64>();

        // 3. Add components for entity id 0
        archetype.add_component::<u32>(entity_1_id, 21);
        archetype.add_component::<u64>(entity_1_id, 212);

        // 4. Add components for entity id 1
        archetype.add_component::<u32>(entity_2_id, 35);
        archetype.add_component::<u64>(entity_2_id, 123);

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

        let entity_1_id = 0;
        let entity_2_id = 10;
        let entity_3_id = 5;

        let entity_1_idx = 0;
        let entity_2_idx = 1;
        let entity_3_idx = 2;

        // 1. Create component vectors for types u32 and u64
        archetype.add_component_vec::<u32>();
        archetype.add_component_vec::<u64>();

        // 2. store entities
        archetype.store_entity(entity_1_id);
        archetype.store_entity(entity_2_id);
        archetype.store_entity(entity_3_id);

        // 3. add components in random order, with values 1-6
        archetype.add_component::<u32>(entity_2_id, 1);
        archetype.add_component::<u64>(entity_1_id, 2);
        archetype.add_component::<u64>(entity_2_id, 3);
        archetype.add_component::<u32>(entity_3_id, 4);
        archetype.add_component::<u32>(entity_1_id, 5);
        archetype.add_component::<u64>(entity_3_id, 6);

        let result_u32 = archetype.borrow_component_vec::<u32>().unwrap();
        let result_u64 = archetype.borrow_component_vec::<u64>().unwrap();

        // 4. assert values are correct
        assert_eq!(result_u32.get(entity_2_idx).unwrap().unwrap(), 1);
        assert_eq!(result_u64.get(entity_1_idx).unwrap().unwrap(), 2);
        assert_eq!(result_u64.get(entity_2_idx).unwrap().unwrap(), 3);
        assert_eq!(result_u32.get(entity_3_idx).unwrap().unwrap(), 4);
        assert_eq!(result_u32.get(entity_1_idx).unwrap().unwrap(), 5);
        assert_eq!(result_u64.get(entity_3_idx).unwrap().unwrap(), 6);
    }

    #[test]
    fn storing_entities_gives_them_indices_when_component_vec_exists() {
        let mut archetype = Archetype::default();

        let entity_1_id = 0;
        let entity_2_id = 27;
        let entity_3_id = 81;
        let entity_4_id = 100;

        // 1. Add component vec
        archetype.add_component_vec::<u8>();

        // 2. Store 4 entities
        archetype.store_entity(entity_1_id);
        archetype.store_entity(entity_2_id);
        archetype.store_entity(entity_3_id);
        archetype.store_entity(entity_4_id);

        let result_u8 = archetype.borrow_component_vec::<u8>().unwrap();

        // The component vec should have a length of 4, since that is the number of entities stored
        assert_eq!(result_u8.len(), 4);
        // No values should be stored since none have been added.
        result_u8.iter().for_each(|v| assert!(v.is_none()));
    }

    #[test]
    fn adding_component_vec_after_entities_have_been_added_gives_entities_indices_in_the_new_vec() {
        let mut archetype = Archetype::default();

        let entity_1_id = 0;
        let entity_2_id = 27;
        let entity_3_id = 81;
        let entity_4_id = 100;

        // 1. Store 4 entities
        archetype.store_entity(entity_1_id);
        archetype.store_entity(entity_2_id);
        archetype.store_entity(entity_3_id);
        archetype.store_entity(entity_4_id);

        // 2. Add component vec
        archetype.add_component_vec::<u64>();

        let result_u64 = archetype.borrow_component_vec::<u64>().unwrap();

        // The component vec should have a length of 4, since that is the number of entities stored
        assert_eq!(result_u64.len(), 4);
        // No values should be stored since none have been added.
        result_u64.iter().for_each(|v| assert!(v.is_none()));
    }

    #[test]
    fn interleaving_adding_vecs_and_storing_entities_results_in_correct_length_of_vecs() {
        let mut archetype = Archetype::default();

        let entity_1_id = 0;
        let entity_2_id = 27;
        let entity_3_id = 81;
        let entity_4_id = 100;

        // 1. add u8 component vec.
        archetype.add_component_vec::<u8>();

        // 2. store entities 1 and 2.
        archetype.store_entity(entity_1_id);
        archetype.store_entity(entity_2_id);

        // 3. add u64 component vec.
        archetype.add_component_vec::<u64>();

        // 4. store entities 3 and 4.
        archetype.store_entity(entity_3_id);
        archetype.store_entity(entity_4_id);

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

    #[test]
    fn borrow_with_signature_returns_expected_values() {
        // Arrange
        let mut world = World::default();

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
        world.store_entity_in_archetype(e1.id, 0);
        world.store_entity_in_archetype(e2.id, 0);
        // e3 is stored in archetype index 1
        world.store_entity_in_archetype(e3.id, 1);

        // insert some components...
        world.add_component::<u64>(e1.id, 1);
        world.add_component::<u32>(e1.id, 2);

        world.add_component::<u64>(e2.id, 3);
        world.add_component::<u32>(e2.id, 4);

        world.add_component::<u32>(e3.id, 6);

        // Act
        // Borrow all vecs containing u32 from archetypes have the signature u32
        let vecs_u32 = world.borrow_component_vecs_with_signature::<u32>(&[TypeId::of::<u32>()]);

        // Assert
        // Collect values from vecs
        let result: Vec<u32> = vecs_u32
            .iter()
            .flat_map(|x| x.as_ref().unwrap().iter().map(|v| v.unwrap()))
            .collect();
        println!("{:?}", result);

        assert_eq!(result, vec![2, 4, 6])
    }

    #[test]
    fn querying_with_archetypes() {
        // Arrange
        let mut world = World::default();

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
        world.store_entity_in_archetype(e1.id, 0);
        world.store_entity_in_archetype(e2.id, 0);
        // e3 is stored in archetype index 1
        world.store_entity_in_archetype(e3.id, 1);

        // insert some components...
        world.add_component::<u64>(e1.id, 1);
        world.add_component::<u32>(e1.id, 2);

        world.add_component::<u64>(e2.id, 3);
        world.add_component::<u32>(e2.id, 4);

        world.add_component::<u32>(e3.id, 6);

        let mut result: Vec<u32> = vec![];

        let mut borrowed = <Read<u32> as SystemParameter>::borrow(
            &world,
            &[<Read<u32> as SystemParameter>::signature()],
        );
        unsafe {
            while let Some(parameter) =
                <Read<u32> as SystemParameter>::fetch_parameter(&mut borrowed)
            {
                if let Some(parameter) = parameter {
                    result.push(*parameter);
                }
            }
        }

        println!("{:?}", result);

        assert_eq!(result, vec![2, 4, 6])
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

        archetype.store_entity(10);
        archetype.store_entity(20);
        archetype.store_entity(30);

        archetype.add_component_vec::<u32>();
        archetype.add_component_vec::<f32>();

        archetype.add_component::<u32>(10, 1);
        archetype.add_component::<u32>(20, 2);
        archetype.add_component::<u32>(30, 3);

        archetype.add_component::<f32>(10, 1.0);
        archetype.add_component::<f32>(20, 2.0);
        archetype.add_component::<f32>(30, 3.0);

        // Act
        archetype.remove_entity(10);

        // Assert
        let component_vec_u32 = archetype.borrow_component_vec::<u32>().unwrap();
        let component_vec_f32 = archetype.borrow_component_vec::<f32>().unwrap();

        // Removing entity_id 10 should move components of entity_id 30 to index 0.
        assert_eq!(component_vec_u32.get(0).unwrap().unwrap(), 3);
        assert_eq!(component_vec_f32.get(0).unwrap().unwrap(), 3.0);
        // Components of entity_id 20 should stay on index 1.
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
    #[test_case(vec![Vec::<usize>::new()], vec![]; "empty")]
    #[test_case(vec![Vec::<usize>::new(), Vec::<usize>::new(), Vec::<usize>::new(), Vec::<usize>::new()], vec![]; "multiple empty")]
    #[test_case(vec![Vec::<usize>::new(), vec![1,2,3,4]], vec![]; "one empty, one not")]
    #[test_case(vec![vec![2,1,1,1,1], vec![1,1,1,1,2], vec![1,1,2,1,1]], vec![2,1,1,1,1]; "multiple of the same number")]
    fn intersection_returns_expected_values(
        test_vecs: Vec<Vec<usize>>,
        expected_value: Vec<usize>,
    ) {
        // Construct test values, to avoid upsetting Rust and test_case
        let borrowed_test_vecs: Vec<&Vec<usize>> = test_vecs.iter().collect();
        let borrowed_expected_value: Vec<&usize> = expected_value.iter().collect();

        // Perform intersection operation
        let result = intersection(borrowed_test_vecs);

        // Assert intersection result equals expected value
        assert_eq!(result, borrowed_expected_value);
    }
}
