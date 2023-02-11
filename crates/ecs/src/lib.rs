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
    clippy::print_stdout,
    clippy::rc_mutex,
    clippy::unwrap_used,
    clippy::large_enum_variant
)]

use std::fmt::Formatter;

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

    pub fn add_system<Sys: System + 'static>(mut self, system: Sys) -> Self {
        self.systems.push(Box::new(system));
        self
    }

    pub fn run(&mut self) {
        for system in &mut self.systems {
            system.run();
        }
    }
}

pub trait System {
    fn run(&mut self);
}

impl<F> System for F
where
    F: Fn(),
{
    fn run(&mut self) {
        self();
    }
}
