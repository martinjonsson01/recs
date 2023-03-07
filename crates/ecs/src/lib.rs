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

use std::any::{Any, TypeId};
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::{RwLock, RwLockReadGuard};
use std::{any, fmt};
use paste::paste;
use crossbeam::channel::{Receiver, TryRecvError};

/// The entry-point of the entire program, containing all of the entities, components and systems.
#[derive(Default, Debug)]
pub struct Application {
    world: World,
    systems: Vec<Box<dyn System>>,
}

impl Application {
    pub fn add_system<System, Parameters>(mut self, system: System) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.systems.push(Box::new(system.into_system()));
        self
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
    components: Vec<Box<dyn ComponentVec>>,
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
    fn run(&self, world: &World);
    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor>;
}

/// What component is accessed and in what manner (read/write).
#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
pub enum ComponentAccessDescriptor {
    Read(TypeId),
    Write(TypeId),
}

/// Something that can be turned into a `ecs::System`.
pub trait IntoSystem<Parameters> {
    type Output: System + 'static;

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
    fn run(&self, world: &World) {
        todo!()
    }

    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor> {
        todo!()
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
pub trait SystemParameters: Send + Sync {}

/// Something that can be passed to a `ecs::System`.
pub trait SystemParameter: Send + Sync + Sized {
    /// Contains a borrow of components from `ecs::World`.
    type BorrowedData<'components>;

    /// Borrows the collection of components of the given type from `ecs::World`.
    fn borrow(world: &World) -> Self::BorrowedData<'_>;

    /// Fetches the parameter from the borrowed data for a given entity.
    /// # Safety
    /// The returned value is only guaranteed to be valid until BorrowedData is dropped
    fn fetch_parameters(borrowed: &mut Self::BorrowedData<'_>) -> Option<Vec<Option<Self>>>;
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

/*todo: impl<Component: Send + Sync + 'static + Sized> SystemParameter for Read<'_, Component> {
    type BorrowedData<'components> = Option<RwLockReadGuard<'components, Vec<Option<Component>>>>;

    fn borrow(world: &World) -> Self::BorrowedData<'_> {
        world.borrow_component_vec::<Component>()
    }

    fn fetch_parameters(borrowed: &mut Self::BorrowedData<'_>) -> Option<Vec<Option<Self>>> {
        if let Some(component_vec) = borrowed {
            comp
        }
        None
    }
}*/

/// A read-only access to a component of the given type.
#[derive(Debug)]
pub struct ReadWrite<'a, Component: 'static> {
    output: &'a mut Component,
}

impl<'a, Component> Deref for ReadWrite<'a, Component> {
    type Target = Component;

    fn deref(&self) -> &Self::Target {
        self.output
    }
}

impl<'a, Component> DerefMut for ReadWrite<'a, Component> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.output
    }
}

impl SystemParameters for () {}
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
            impl<$([<P$parameter>]: SystemParameter,)*> SystemParameters for ($([<P$parameter>],)*) {}
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

/*impl_system_parameter_function!(0);
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
impl_system_parameter_function!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);*/

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn olle() {
        let i: ComponentVecImpl<u32> = Default::default();
        println!("i = {:?}", i);
    }
}
