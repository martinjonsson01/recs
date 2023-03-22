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
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};
use std::{any, fmt, iter};
use tracing::instrument;

/// The entry-point of the entire program, containing all of the entities, components and systems.
#[derive(Default, Debug)]
pub struct Application {
    world: World,
    systems: Vec<Box<dyn System>>,
}

impl Application {
    /// Spawns a new entity in the world.
    pub fn create_entity(&mut self) -> Entity {
        for component_vec in self.world.component_vecs_hash_map.values_mut() {
            component_vec.push_none();
        }
        let entity = Entity {
            id: self.world.entities.len(),
            _generation: 0,
        };
        self.world.entities.push(entity);
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
        let component_typeid = TypeId::of::<ComponentType>();
        if let Some(component_vec) = self.world.component_vecs_hash_map.get_mut(&component_typeid) {
            if let Some(component_vec) = component_vec
                .as_any_mut()
                .downcast_mut::<ComponentVecImpl<ComponentType>>()
            {
                component_vec.write().expect("Lock is poisoned")[entity.id] = Some(component);
                return;
            }
        }

        self.world.create_component_vec_and_add(entity, component);
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

/// Represents the simulated world.
#[derive(Default, Debug)]
pub struct World {
    entities: Vec<Entity>,
    entity_to_generation: HashMap<usize, usize>,
    component_vecs_hash_map: HashMap<TypeId, Box<dyn ComponentVec>>,
    
    component_vecs: Vec<Box<dyn ComponentVec>>,
    component_typeid_to_component_vec_indices: HashMap<TypeId, Vec<usize>>,
    
    signature_to_vec_indices: HashMap<u64, Vec<usize>>,
    
    
    signature_len: HashMap<u64, usize>,
    
    entity_id_to_signature_and_component_index: HashMap<usize,(u64, usize)>,
    // signature_and_entity_id_to_component_index: HashMap<(u64, usize), usize>,

}

type ReadComponentVec<'a, ComponentType> = Option<RwLockReadGuard<'a, Vec<Option<ComponentType>>>>;
type WriteComponentVec<'a, ComponentType> =
    Option<RwLockWriteGuard<'a, Vec<Option<ComponentType>>>>;

impl World {
    fn create_component_vec_and_add<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        // let (signature, component_index) = self.entity_id_to_signature_and_component_index.get(&entity.id).expect("entity does not exist");
        
        
        self.add_component(&entity.id, component);
        

        // let mut new_component_vec = Vec::with_capacity(self.entities.len());

        // for _ in &self.entities {
        //     new_component_vec.push(None)
        // }

        // new_component_vec[entity.id] = Some(component);

        // let component_typeid = TypeId::of::<ComponentType>();
        // self.component_vecs_hash_map
        // .insert(component_typeid, Box::new(RwLock::new(new_component_vec)));
    }

    fn create_new_entity(&mut self) -> Entity {
        let entity_id = self.entities.len();
        self.entity_to_generation.insert(entity_id, 0);
        Entity {id: entity_id, _generation: 0}
    }

    fn add_component_vec<ComponentType: Debug + Send + Sync + 'static>(&mut self) {
        let component_typeid = TypeId::of::<ComponentType>();
        let raw_component_vec = create_raw_component_vec::<ComponentType>();
        self.add_raw_component_vec(raw_component_vec, component_typeid);
    }
    
    fn add_raw_component_vec(&mut self, raw_component_vec: Box<dyn ComponentVec>, component_typeid: TypeId) -> usize {
        let component_vec_index = self.component_vecs.len();

        if let Some(vec_indices) = self.component_typeid_to_component_vec_indices.get_mut(&component_typeid) {
            vec_indices.push(component_vec_index);
        } else {
            self.component_typeid_to_component_vec_indices.insert(component_typeid, vec![component_vec_index]);
        }

        self.component_vecs.push(raw_component_vec);
        component_vec_index.clone()
    }

    fn add_archetype(&mut self, archetype_builder: ArchetypeBuilder) {
        let mut component_typeids: Vec<TypeId> = archetype_builder.vecs.iter().map(|(component_typeid, _)| component_typeid.clone()).collect();

        let signature = generate_signature(&mut component_typeids);

        let component_vec_indices: Vec<usize> = archetype_builder.vecs
            .into_iter()
            .map(|(component_typeid, component_vec)| {
                self.add_raw_component_vec(component_vec, component_typeid)
            }).collect();

        self.signature_to_vec_indices.insert(signature, component_vec_indices); 
        self.signature_len.insert(signature, 0);
    }

    /// Assigns to which signature the components of the specified entity will be stored
    fn assign_entity_to_signature(&mut self, entity_id: usize, signature: u64) {    
        
        // check if entity exists
        if !self.entity_to_generation.contains_key(&entity_id) {
            panic!("Entity does not exist!")
        }

        if let Some(_old_signature) = self.entity_id_to_signature_and_component_index.get(&entity_id) {
            todo!() // Move components to new signature
        }

        let signature_vec_indices = self.signature_to_vec_indices.get(&signature).expect("Signature does not exist");

        signature_vec_indices.iter().for_each(|&idx| self.component_vecs.get_mut(idx).expect("Component vec is missing").push_none());
        let &component_index = self.signature_len.get(&signature).expect("Signature length is missing");

        // self.signature_and_entity_id_to_component_index.insert((signature, entity_id), component_index);
        self.entity_id_to_signature_and_component_index.insert(entity_id, (signature, component_index));
    }

    fn add_component<ComponentType: Debug + Send + Sync + 'static>(&self, entity_id: &usize, component: ComponentType) {
        if let Some((signature, component_index)) = self.entity_id_to_signature_and_component_index.get(&entity_id) {
            let mut component_vec = self.get_component_vec_mut::<ComponentType>(signature).expect("Signature component vec is missing");
            component_vec[*component_index] = Some(component);
        } else {
            panic!("Entity is not assigned to a signature!")
        }
    }
    
    fn get_component_vec<ComponentType: Debug + Send + Sync + 'static>(&self, signature: &u64) -> ReadComponentVec<ComponentType>{
        let component_typeid = TypeId::of::<ComponentType>();
        
        if let Some(signature_indices) = self.signature_to_vec_indices.get(&signature) {
            if let Some(component_indices) = self.component_typeid_to_component_vec_indices.get(&component_typeid) {
                let indices = signature_indices.intersection(component_indices);
                if indices.len() == 1 {
                    let &component_vec_index = indices.first().expect("index missing");
                    return borrow_component_vec::<ComponentType>(self.component_vecs.get(component_vec_index).expect("component vec is missing"));
                } else {
                    panic!("More than one component vec was found for a single signature!")
                }
            }
        }
        None
    }

    fn get_component_vec_mut<ComponentType: Debug + Send + Sync + 'static>(&self, signature: &u64) -> WriteComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        
        if let Some(signature_indices) = self.signature_to_vec_indices.get(&signature) {
            if let Some(component_indices) = self.component_typeid_to_component_vec_indices.get(&component_typeid) {
                let indices = signature_indices.intersection(component_indices);
                if indices.len() == 1 {
                    let &component_vec_index = indices.first().expect("Entity component index missing for component {component_typeid}, signature: {signature}");
                    return borrow_component_vec_mut::<ComponentType>(self.component_vecs.get(component_vec_index).expect("component vec is missing"));
                } else {
                    panic!("More than one component vec was found for a single signature!")
                }
            }
        }
        None
    }

    fn borrow_component_vec<ComponentType: 'static>(&self) -> ReadComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        if let Some(component_vec) = self.component_vecs_hash_map.get(&component_typeid) {
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
        }
        None
    }

    fn borrow_component_vec_mut<ComponentType: 'static>(&self) -> WriteComponentVec<ComponentType> {
        let component_typeid = TypeId::of::<ComponentType>();
        if let Some(component_vec) = self.component_vecs_hash_map.get(&component_typeid) {
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
        }
        None
        // if let Some(indices) = self.typeid_to_component_vec_indices.get() {
            //     self.get_borrow_iterator_mut(indices);
            // }
        }
        
        fn get_borrow_iterator<'a, ComponentType: Debug + Send + Sync + 'static>(&'a self, indices: &'a Vec<usize>)  -> impl Iterator<Item = ReadComponentVec<ComponentType>> + 'a {
            indices.iter().map(|&component_vec_index| borrow_component_vec::<ComponentType>(self.component_vecs.get(component_vec_index).expect("Component vec index does not exist")))
        }
        
        fn get_borrow_iterator_mut<'a, ComponentType: Debug + Send + Sync + 'static>(&'a self, indices: &'a Vec<usize>)  -> impl Iterator<Item = WriteComponentVec<ComponentType>> + 'a {
            indices.iter().map(|&component_vec_index| borrow_component_vec_mut::<ComponentType>(self.component_vecs.get(component_vec_index).expect("Component vec index does not exist")))
    }
}

fn panic_locked_component_vec<ComponentType: 'static>() -> ! {
    let component_type_name = any::type_name::<ComponentType>();
    panic!(
        "Lock of ComponentVec<{}> is already taken!",
        component_type_name
    )
}

struct ArchetypeBuilder {
    vecs: Vec<(TypeId, Box<dyn ComponentVec>)>
}

impl ArchetypeBuilder {
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(&mut self) {
        let component_typeid = TypeId::of::<ComponentType>();
        self.vecs.push((component_typeid, create_raw_component_vec::<ComponentType>()))
    }

    fn build(self) -> Vec<(TypeId, Box<dyn ComponentVec>)>{
        self.vecs
    }
}

fn generate_signature(component_typeids: &mut Vec<TypeId>) -> u64 {
    component_typeids.sort();
    let mut hasher = DefaultHasher::new();
    component_typeids.iter().for_each(|x| x.hash(&mut hasher));
    hasher.finish()
}


fn create_raw_component_vec<ComponentType: Debug + Send + Sync + 'static>() -> Box<dyn ComponentVec>{
    Box::new(RwLock::new(Vec::<Option<ComponentType>>::new()))
}

fn from_raw_component_vec<ComponentType: Debug + Send + Sync + 'static>(component_vec: &Box<dyn ComponentVec>) -> &ComponentVecImpl<ComponentType> {
    component_vec.as_any().downcast_ref::<ComponentVecImpl<ComponentType>>().expect("could not downcast raw component vec")
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

trait Intersectable<T: PartialEq + Copy> {
    fn intersection(&self, other: &Vec<T>) -> Vec<T>;
}

impl<T: PartialEq + Copy> Intersectable<T> for Vec<T> {
    fn intersection(&self, other: &Vec<T>) -> Vec<T> {
        self.iter().filter(|&x| other.contains(x)).map(|x| *x).collect()
    }
}

type ComponentVecImpl<ComponentType> = RwLock<Vec<Option<ComponentType>>>;

trait ComponentVec: Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn push_none(&mut self);
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

impl<Component: Send + Sync + 'static + Sized> SystemParameter for Read<'_, Component> {
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

impl<Component: Send + Sync + 'static + Sized> SystemParameter for Write<'_, Component> {
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
}
