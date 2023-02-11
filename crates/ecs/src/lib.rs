//! A barebones ECS-implementation for the scheduler prototype.

// rustc lints
#![warn(
    let_underscore,
    nonstandard_style,
    unused,
    explicit_outlives_requirements,
    meta_variable_misuse,
    missing_debug_implementations,
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
    clippy::rc_mutex,
    clippy::unwrap_used,
    clippy::large_enum_variant
)]

use std::fmt::Formatter;
use std::marker::PhantomData;

/// A container for the `ecs::System`s that run in the application.
#[derive(Debug, Default)]
pub struct World {
    systems: Vec<Box<dyn System>>,
}

impl std::fmt::Debug for dyn System + 'static {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "system")
    }
}

impl World {
    /// Creates a new world.
    pub fn new() -> World {
        World::default()
    }

    pub fn add_system<F: IntoSystem<Parameters>, Parameters: SystemParameter>(
        mut self,
        function: F,
    ) -> Self {
        self.systems.push(Box::new(function.into_system()));
        self
    }

    pub fn run(&mut self) {
        for system in &mut self.systems {
            system.run();
        }
    }
}

#[derive(Debug)]
pub struct Query<T> {
    _output: T,
}

pub trait System {
    fn run(&mut self);
}

pub trait IntoSystem<Parameters> {
    type Output: System + 'static;

    fn into_system(self) -> Self::Output;
}

#[derive(Debug)]
pub struct FunctionSystem<F, Parameters: SystemParameter> {
    system: F,
    parameters: PhantomData<Parameters>,
}

impl<F, Parameters: SystemParameter> System for FunctionSystem<F, Parameters>
where
    F: SystemParameterFunction<Parameters> + 'static,
{
    fn run(&mut self) {
        SystemParameterFunction::run(&mut self.system);
    }
}

impl<F, Parameters: SystemParameter + 'static> IntoSystem<Parameters> for F
where
    F: SystemParameterFunction<Parameters> + 'static,
{
    type Output = FunctionSystem<F, Parameters>;

    fn into_system(self) -> Self::Output {
        FunctionSystem {
            system: self,
            parameters: PhantomData,
        }
    }
}

pub trait SystemParameter {}

impl<T> SystemParameter for Query<T> {}
impl SystemParameter for () {}
impl<P0: SystemParameter> SystemParameter for (P0,) {}
impl<P0: SystemParameter, P1: SystemParameter> SystemParameter for (P0, P1) {}

trait SystemParameterFunction<Parameters: SystemParameter>: 'static {
    fn run(&mut self);
}

impl<F> SystemParameterFunction<()> for F
where
    F: Fn() + 'static,
{
    fn run(&mut self) {
        println!("running system with no parameters");
        self();
    }
}

impl<F, P0: SystemParameter> SystemParameterFunction<(P0,)> for F
where
    F: Fn(P0) + 'static,
{
    fn run(&mut self) {
        eprintln!("running system with parameter");
    }
}

impl<F, P0: SystemParameter, P1: SystemParameter> SystemParameterFunction<(P0, P1)> for F
where
    F: Fn(P0, P1) + 'static,
{
    fn run(&mut self) {
        eprintln!("running system with two parameters");
    }
}
