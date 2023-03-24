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

use crossbeam::channel::{Receiver, TryRecvError};
use paste::paste;
use core::panic;
use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};
use std::{any, fmt, iter, clone};
use tracing::instrument;

/// The entry-point of the entire program, containing all of the entities, components and systems.
#[derive(Default, Debug)]
pub struct Application {
    world: World,
    systems: Vec<Box<dyn System>>,
}

impl Application {
    // TODO: Remove. Temporary code for working with archetypes as if they were the Good ol' ComponentVecs implementation.
    /// Returns default values of Application with World containing single Archetype.
    pub fn default() -> Self {
        Self {
            world: World::default(),
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
    pub fn run<'systems, E: Executor, S: Schedule<'systems>>(
        &'systems mut self,
        shutdown_receiver: Receiver<()>,
    ) {
        let schedule = S::generate(&self.systems);
        let mut executor = E::default();
        executor.execute(schedule, &self.world, shutdown_receiver);
    }
}

/// A way of executing a `ecs::Schedule`.
pub trait Executor: Default {
    /// Executes systems in a world according to a given schedule.
    fn execute<'a, S: Schedule<'a>>(
        &mut self,
        schedule: S,
        world: &World,
        shutdown_receiver: Receiver<()>,
    );
}

/// Runs systems in sequence, one after the other.
#[derive(Default, Debug)]
pub struct Sequential;

impl Executor for Sequential {
    fn execute<'systems, S: Schedule<'systems>>(
        &mut self,
        mut schedule: S,
        world: &World,
        shutdown_receiver: Receiver<()>,
    ) {
        while let Err(TryRecvError::Empty) = shutdown_receiver.try_recv() {
            for batch in schedule.next_batch() {
                batch.run(world);
            }
        }
    }
}

/// An ordering of `ecs::System` executions.
pub trait Schedule<'systems> {
    /// Creates a scheduling of the given systems.
    fn generate(systems: &'systems [Box<dyn System>]) -> Self;

    /// Gets the next batch of systems that are safe to execute concurrently.
    fn next_batch(&mut self) -> Vec<&dyn System>;
}

/// Schedules systems in no particular order, with no regard to dependencies.
#[derive(Default, Debug)]
pub struct Unordered<'systems>(&'systems [Box<dyn System>]);

impl<'systems> Schedule<'systems> for Unordered<'systems> {
    fn generate(systems: &'systems [Box<dyn System>]) -> Self {
        Self(systems)
    }

    fn next_batch(&mut self) -> Vec<&dyn System> {
        self.0.iter().map(|system| system.as_ref()).collect()
    }
}

#[derive(Debug, Default)]
struct Archetype {
    component_typeid_to_component_vec: HashMap<TypeId, Box<dyn ComponentVec>>,
    entity_id_to_component_index: HashMap<usize, usize>,
}

impl Archetype {
    /// Adds an `entity_id` to keep track of and store components for.
    fn store_entity(&mut self, entity_id: usize) {
        let entity_index = self.entity_id_to_component_index.len();

        self.component_typeid_to_component_vec.values_mut().into_iter().for_each(|v| v.push_none());

        self.entity_id_to_component_index.insert(entity_id, entity_index);
    }
    
    /// Returns a `ReadComponentVec` with the specified generic type `ComponentType` if it is stored. 
    fn borrow_component_vec<ComponentType: Debug + Send + Sync + 'static>(&self) -> ReadComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        if let Some(component_vec) = self.component_typeid_to_component_vec.get(&component_typeid) {
            return borrow_component_vec(component_vec);
        }
        None
    }

    fn borrow_component_vec_mut<ComponentType: Debug + Send + Sync + 'static>(&self) -> WriteComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        if let Some(component_vec) = self.component_typeid_to_component_vec.get(&component_typeid) {
            return borrow_component_vec_mut(component_vec);
        }
        None
    }

    /// Adds a component of type `ComponentType` to the specified `entity`.
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(&mut self, entity_id: usize, component: ComponentType) {
        if let Some(&entity_index) = self.entity_id_to_component_index.get(&entity_id) {
            if let Some(mut component_vec) = self.borrow_component_vec_mut::<ComponentType>(){ 
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
            self.component_typeid_to_component_vec.insert(component_typeid, raw_component_vec);
        }
    }

    /// Returns `true` if the archetype stores components of type ComponentType.
    fn contains<ComponentType: Debug + Send + Sync + 'static>(&self) -> bool {
        self.component_typeid_to_component_vec.contains_key(&TypeId::of::<ComponentType>())
    }
}

/// Represents the simulated world.
#[derive(Default, Debug)]
pub struct World {
    entities: Vec<Entity>,
    entity_id_to_archetype_index: HashMap<usize, usize>,
    archetypes: Vec<Archetype>,
    component_typeid_to_archetype_indices: HashMap<TypeId, Vec<usize>>,
    stored_types: Vec<TypeId>, // TODO: Remove. Used to showcase how archetypes can be used in querying. 
}

type ReadComponentVec<'a, ComponentType> = Option<RwLockReadGuard<'a, Vec<Option<ComponentType>>>>;
type WriteComponentVec<'a, ComponentType> =
Option<RwLockWriteGuard<'a, Vec<Option<ComponentType>>>>;

impl World {
    // TODO: Remove. Temporary code for working with archetypes as if they were the Good ol' ComponentVecs implementation. 
    /// Returns World with single premade Archetype, corresponding to the Good ol' ComponentVecs
    pub fn default() -> Self{
        Self{
            archetypes: vec![Archetype::default()],
            ..Default::default()
        }
    }

    fn create_component_vec_and_add<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        let big_archetype = match self.archetypes.get_mut(0) {
            Some(big_archetype) => big_archetype,
            None => { self.add_empty_archetype(Archetype::default()); self.archetypes.get_mut(0).expect("just added") } ,
        };

        if !big_archetype.contains::<ComponentType>() {
            let component_typeid = TypeId::of::<ComponentType>();
            self.stored_types.push(component_typeid);
            // ↓↓↓↓ copied code from fn add_empty_archetype(...) ↓↓↓↓
            let archetype_index = 0;
            match self.component_typeid_to_archetype_indices.get_mut(&component_typeid) {
                Some(indices) => indices.push(archetype_index),
                None => { self.component_typeid_to_archetype_indices.insert(component_typeid, vec![archetype_index]); },
            }
            // ↑↑↑↑ copied code from fn add_empty_archetype(...) ↑↑↑↑ 

        }
        
        big_archetype.add_component_vec::<ComponentType>();

        self.add_component(entity.id, component);
    }

    fn create_new_entity(&mut self) -> Entity {
        let entity_id = self.entities.len();
        let entity = Entity {id: entity_id, _generation: 0};
        self.entities.push(entity);
        entity
    }
    
    fn add_empty_archetype(&mut self, archetype: Archetype) {
        let archetype_index = self.archetypes.len();
        
        archetype.component_typeid_to_component_vec.values().into_iter().for_each(|v| {
            let component_typeid = v.stored_type();
            match self.component_typeid_to_archetype_indices.get_mut(&component_typeid) {
                Some(indices) => indices.push(archetype_index),
                None => { self.component_typeid_to_archetype_indices.insert(component_typeid, vec![archetype_index]); },
            }
        });

        self.archetypes.push(archetype);
    }

    fn store_entity_in_archetype(&mut self, entity_id: usize, archetype_index: usize) {
        let archetype = self.archetypes.get_mut(archetype_index).expect("Archetype does not exist");

        archetype.store_entity(entity_id);

        if self.entity_id_to_archetype_index.contains_key(&entity_id) {
            todo!("Add code for cleaning up old archetype");
        }

        self.entity_id_to_archetype_index.insert(entity_id, archetype_index);
    }

    fn add_component<ComponentType: Debug + Send + Sync + 'static>(&mut self, entity_id: usize, component: ComponentType) {
        if let Some(&archetype_index) = self.entity_id_to_archetype_index.get(&entity_id) {
            let archetype = self.archetypes.get_mut(archetype_index).expect("Archetype is missing");
            archetype.add_component::<ComponentType>(entity_id, component);
        }
    }

    fn borrow_component_vec<ComponentType: Debug + Send + Sync + 'static>(&self) -> ReadComponentVec<ComponentType> {
        // TODO: Remove. Temporary code for working with archetypes as if they were the Good ol' ComponentVecs implementation.
        
        // Comment: Ugly code that shows off the idea of the function 
        // get_archetypes_with().
        
        // What it does: Finds all archetypes that contain all of the specified types. 
        // In this temporary version the types that the systems want to query 
        // are stored in 'stored_types', which is a vector that contain all of 
        // the types that have been added to the single "large" archetype.
        
        let mut a = self.get_archetypes_with(&self.stored_types);
        
        if a.len() == 1 {
            return a.pop().unwrap().borrow_component_vec::<ComponentType>();
        } else {
            // When there exists multiple archetypes, this function can no longer be used.
            // A function which returns either a Vec or Iterator of 
            // ReadComponentVec<ComponentType> will need to be added.
            todo!();
        }
        // Above code is basically equivalent to this below:
        // if let Some(large_archetype) = self.archetypes.get(0) {
        //     return large_archetype.borrow_component_vec::<ComponentType>();
        // }
    }

    fn borrow_component_vec_mut<ComponentType: Debug + Send + Sync + 'static>(&self) -> WriteComponentVec<ComponentType> {
        // TODO: Remove. Temporary code for working with archetypes as if they were the Good ol' ComponentVecs implementation.
        // See comments above
        let mut a = self.get_archetypes_with(&self.stored_types);
        
        if a.len() == 1 {
            return a.pop().unwrap().borrow_component_vec_mut::<ComponentType>();
        } else {
            todo!();
        }
    }

    fn get_archetypes_with(&self, signature: &Vec<TypeId>) -> Vec<&Archetype>{
        // Selects all archetypes that contain the types specified in signature.
        // Ex. if the signature is (A,B,C) then we will find the indices of
        // archetypes: (A), (A,B), (C), (A,B,C,D), because they all containe 
        // some of the types from the signature.
        let all_archetypes_with_signature_types: Vec<&Vec<usize>> = signature.iter().map(|x| self.component_typeid_to_archetype_indices.get(x).unwrap()).collect();
        
        // Select only the archetypes that contain all of the types in signature.
        // Ex. continuing with the example above, where the signature is (A,B,C)
        // only the archetype (A,B,C,D) will be returned. 
        let only_components_with_signature_types = intersection(all_archetypes_with_signature_types);
        
        // Selects the archetypes by using the indices
        let found_archetypes: Vec<&Archetype> = only_components_with_signature_types.iter().map(|&&x| self.archetypes.get(x).unwrap()).collect();

        found_archetypes
    }

    // Examples of how an iterator could be implemented:
    fn get_borrow_iterator<'a, ComponentType: Debug + Send + Sync + 'static>(&'a self, archetype_indices: &'a Vec<&usize>)  -> impl Iterator<Item = ReadComponentVec<ComponentType>> + 'a {
        archetype_indices.iter().map(|&&archetype_index| self.archetypes.get(archetype_index).expect("Archetype does not exist").borrow_component_vec())
    }
    
    fn get_borrow_iterator_mut<'a, ComponentType: Debug + Send + Sync + 'static>(&'a self, archetype_indices: &'a Vec<usize>)  -> impl Iterator<Item = WriteComponentVec<ComponentType>> + 'a {
        archetype_indices.iter().map(|&archetype_index| self.archetypes.get(archetype_index).expect("Archetype does not exist").borrow_component_vec_mut())
    }
}

fn panic_locked_component_vec<ComponentType: 'static>() -> ! {
    let component_type_name = any::type_name::<ComponentType>();
    panic!(
        "Lock of ComponentVec<{}> is already taken!",
        component_type_name
    )
}

fn create_raw_component_vec<ComponentType: Debug + Send + Sync + 'static>() -> Box<dyn ComponentVec>{
    Box::new(RwLock::new(Vec::<Option<ComponentType>>::new()))
}

fn borrow_component_vec<ComponentType: 'static>(component_vec: &Box<dyn ComponentVec>) -> ReadComponentVec<ComponentType> {
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

fn borrow_component_vec_mut<ComponentType: 'static>(component_vec: &Box<dyn ComponentVec>) -> WriteComponentVec<ComponentType> {
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
    head.into_iter().filter(|x| tail.iter().all(|v| v.contains(x))).collect() 
}

type ComponentVecImpl<ComponentType> = RwLock<Vec<Option<ComponentType>>>;

trait ComponentVec: Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn push_none(&mut self);
    fn stored_type(&self) -> TypeId;
    fn len(&self) -> usize; 
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
    /// Executes the system on each entity matching its query.
    ///
    /// Systems that do not query anything run once per tick.
    fn run(&self, world: &World);
    /// Which component types the system accesses and in what manner (read/write).
    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor>;
}

/// What component is accessed and in what manner (read/write).
#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
pub enum ComponentAccessDescriptor {
    /// Reads from component of provided type.
    Read(TypeId),
    /// Reads and writes from component of provided type.
    Write(TypeId),
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
    parameters: PhantomData<Parameters>,
}

impl<Function: Send + Sync, Parameters: SystemParameters> Debug
    for FunctionSystem<Function, Parameters>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let function_name = any::type_name::<Function>();
        let colon_index = function_name.rfind("::").ok_or(fmt::Error::default())?;
        let system_name = &function_name[colon_index + 2..];

        let parameters_name = any::type_name::<Parameters>();
        let mut parameter_names_text = String::with_capacity(parameters_name.len());
        for parameter_name in parameters_name.split(',') {
            parameter_names_text.push_str(parameter_name);
        }

        writeln!(f, "FunctionSystem {{")?;
        writeln!(f, "    system = {system_name}")?;
        writeln!(f, "    parameters = {parameter_names_text}")?;
        writeln!(f, "}}")
    }
}

impl<Function, Parameters> System for FunctionSystem<Function, Parameters>
where
    Function: SystemParameterFunction<Parameters> + Send + Sync + 'static,
    Parameters: SystemParameters,
{
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
        FunctionSystem {
            function: self,
            parameters: PhantomData,
        }
    }
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
    fn borrow(world: &World) -> Self::BorrowedData<'_>;

    /// Fetches the parameter from the borrowed data for a given entity.
    /// # Safety
    /// The returned value is only guaranteed to be valid until BorrowedData is dropped
    unsafe fn fetch_parameter(
        borrowed: &mut Self::BorrowedData<'_>,
        entity: Entity,
    ) -> Option<Self>;

    /// A description of what data is accessed and how (read/write).
    fn component_access() -> ComponentAccessDescriptor;
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

impl<Component: Debug + Send + Sync + 'static + Sized> SystemParameter for Read<'_, Component> {
    type BorrowedData<'components> = ReadComponentVec<'components, Component>;

    fn borrow(world: &World) -> Self::BorrowedData<'_> {
        world.borrow_component_vec::<Component>()
    }

    unsafe fn fetch_parameter(
        borrowed: &mut Self::BorrowedData<'_>,
        entity: Entity,
    ) -> Option<Self> {
        if let Some(component_vec) = borrowed {
            if let Some(Some(component)) = component_vec.get(entity.id) {
                return Some(Self {
                    // The caller is responsible to only use the
                    // returned value when BorrowedData is still in scope.
                    #[allow(trivial_casts)]
                    output: &*(component as *const Component),
                });
            }
        }
        None
    }

    fn component_access() -> ComponentAccessDescriptor {
        ComponentAccessDescriptor::Read(TypeId::of::<Component>())
    }
}

impl<Component: Debug + Send + Sync + 'static + Sized> SystemParameter for Write<'_, Component> {
    type BorrowedData<'components> = WriteComponentVec<'components, Component>;

    fn borrow(world: &World) -> Self::BorrowedData<'_> {
        world.borrow_component_vec_mut::<Component>()
    }

    unsafe fn fetch_parameter(
        borrowed: &mut Self::BorrowedData<'_>,
        entity: Entity,
    ) -> Option<Self> {
        if let Some(ref mut component_vec) = borrowed {
            if let Some(Some(component)) = component_vec.get_mut(entity.id) {
                return Some(Self {
                    // The caller is responsible to only use the
                    // returned value when BorrowedData is still in scope.
                    #[allow(trivial_casts)]
                    output: &mut *(component as *mut Component),
                });
            }
        }
        None
    }

    fn component_access() -> ComponentAccessDescriptor {
        ComponentAccessDescriptor::Write(TypeId::of::<Component>())
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
                    $(let mut [<borrowed_$parameter>] = <[<P$parameter>] as SystemParameter>::borrow(world);)*

                    for &entity in &world.entities {
                        // SAFETY: This is safe because the result from fetch_parameter will not outlive borrowed
                        unsafe {
                            if let ($(Some([<parameter_$parameter>]),)*) = (
                                $(<[<P$parameter>] as SystemParameter>::fetch_parameter(&mut [<borrowed_$parameter>], entity),)*
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
        let mut world = World::default();

        let entity = Entity {
            id: 0,
            _generation: 1,
        };
        world.entities.push(entity);

        world.create_component_vec_and_add(entity, A);

        let _first = world.borrow_component_vec_mut::<A>();
        let _second = world.borrow_component_vec_mut::<A>();
    }

    #[test]
    fn world_doesnt_panic_when_mutably_borrowing_components_after_dropping_previous_mutable_borrow()
    {
        let mut world = World::default();

        let entity = Entity {
            id: 0,
            _generation: 1,
        };
        world.entities.push(entity);

        world.create_component_vec_and_add(entity, A);

        let first = world.borrow_component_vec_mut::<A>();
        drop(first);
        let _second = world.borrow_component_vec_mut::<A>();
    }

    #[test]
    fn world_does_not_panic_when_trying_to_immutably_borrow_same_components_twice() {
        let mut world = World::default();

        let entity = Entity {
            id: 0,
            _generation: 1,
        };
        world.entities.push(entity);

        world.create_component_vec_and_add(entity, A);

        let _first = world.borrow_component_vec::<A>();
        let _second = world.borrow_component_vec::<A>();
    }

    #[test_case(|_: Read<A>| {}, vec![ComponentAccessDescriptor::Read(TypeId::of::<A>())]; "when reading")]
    #[test_case(|_: Write<A>| {}, vec![ComponentAccessDescriptor::Write(TypeId::of::<A>())]; "when writing")]
    #[test_case(|_: Read<A>, _:Read<B>| {}, vec![ComponentAccessDescriptor::Read(TypeId::of::<A>()), ComponentAccessDescriptor::Read(TypeId::of::<B>())]; "when reading two components")]
    #[test_case(|_: Write<A>, _:Write<B>| {}, vec![ComponentAccessDescriptor::Write(TypeId::of::<A>()), ComponentAccessDescriptor::Write(TypeId::of::<B>())]; "when writing two components")]
    #[test_case(|_: Read<A>, _: Write<B>| {}, vec![ComponentAccessDescriptor::Read(TypeId::of::<A>()), ComponentAccessDescriptor::Write(TypeId::of::<B>())]; "when reading and writing to components")]
    #[test_case(|_: Read<A>, _: Read<B>, _: Read<C>| {}, vec![ComponentAccessDescriptor::Read(TypeId::of::<A>()), ComponentAccessDescriptor::Read(TypeId::of::<B>()), ComponentAccessDescriptor::Read(TypeId::of::<C>())]; "when reading three components")]
    fn component_accesses_return_actual_component_accesses<Params>(
        system: impl IntoSystem<Params>,
        expected_accesses: Vec<ComponentAccessDescriptor>,
    ) {
        let component_accesses = system.into_system().component_accesses();

        assert_eq!(expected_accesses, component_accesses)
    }

    // Archetype tests:

    #[test]
    fn archetype_can_store_components_of_entites_it_stores() {
        let mut archetype = Archetype::default();

        let entity_1_id = 0;
        let entity_2_id = 10;

        // 1. Archetype stores the components of entites with id 0 and id 10
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

        // 2. store entites
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
    fn storing_entities_gives_them_indices_when_componet_vec_exists() {
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
    fn adding_component_vec_after_entites_have_been_added_gives_entities_indices_in_the_new_vec() {
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
    fn interleaving_adding_vecs_and_storing_entites_results_in_correct_length_of_vecs() {
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
        
        // 4. store entites 3 and 4.
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



    // Intersection tests:
    #[test_case(vec![vec![1,2,3]], vec![1,2,3]; "self intersection")]
    #[test_case(vec![vec![1,2,3], vec![1,2,3]], vec![1,2,3]; "two of the same")]
    #[test_case(vec![vec![1], vec![2,3], vec![4]], vec![]; "no overlap, no matches")]
    #[test_case(vec![vec![1,2], vec![2,3], vec![3,4]], vec![]; "some overlap, no matches")]
    #[test_case(vec![vec![1,2,3,4], vec![2,3], vec![3,4]], vec![3]; "some matches")]
    #[test_case(vec![Vec::<usize>::new()], vec![]; "empty")]
    #[test_case(vec![Vec::<usize>::new(), Vec::<usize>::new(), Vec::<usize>::new(), Vec::<usize>::new()], vec![]; "mutliple empty")]
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
