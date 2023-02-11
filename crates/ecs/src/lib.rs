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

mod storage;

use crate::storage::{ComponentVec, ComponentVecImpl};
use std::fmt::Formatter;
use std::iter;
use std::marker::PhantomData;

impl std::fmt::Debug for dyn System + 'static {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "system")
    }
}

/// A container for the `ecs::System`s that run in the application.
#[derive(Debug, Default)]
pub struct World {
    entity_count: usize,
    component_vecs: Vec<Box<dyn ComponentVec>>,
    systems: Vec<Box<dyn System>>,
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Entity(usize);

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

    pub fn new_entity(&mut self) -> Entity {
        let entity_id = self.entity_count;
        for component_vec in self.component_vecs.iter_mut() {
            component_vec.push_none();
        }
        self.entity_count += 1;
        Entity(entity_id)
    }

    pub fn add_component_to_entity<ComponentType: 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        for component_vec in self.component_vecs.iter_mut() {
            if let Some(component_vec) = component_vec
                .as_any_mut()
                .downcast_mut::<ComponentVecImpl<ComponentType>>()
            {
                component_vec[entity.0] = Some(component);
                return;
            }
        }

        self.create_component_vec_and_add(entity, component);
    }

    fn create_component_vec_and_add<ComponentType: 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        let mut new_component_vec: ComponentVecImpl<ComponentType> =
            Vec::with_capacity(self.entity_count);

        for _ in 0..self.entity_count {
            new_component_vec.push(None)
        }

        new_component_vec[entity.0] = Some(component);
        self.component_vecs.push(Box::new(new_component_vec))
    }

    fn borrow_component_vec<ComponentType: 'static>(
        &self,
    ) -> Option<&ComponentVecImpl<ComponentType>> {
        for component_vec in self.component_vecs.iter() {
            if let Some(component_vec) = component_vec
                .as_any()
                .downcast_ref::<ComponentVecImpl<ComponentType>>()
            {
                return Some(component_vec);
            }
        }
        None
    }

    pub fn iterate<ComponentType: 'static>(&self) -> Box<dyn Iterator<Item = &ComponentType> + '_> {
        match self.borrow_component_vec::<ComponentType>() {
            Some(data) => Box::new(data.iter().filter_map(|component| component.as_ref())),
            None => Box::new(iter::empty()),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
    struct TestComponent(i32);

    #[test]
    fn iterate_inserted_component() {
        let mut world = World::new();
        let entity = world.new_entity();
        let component_data = TestComponent(218);
        world.add_component_to_entity(entity, component_data);

        let first_component_data = world.iterate().next().unwrap();

        assert_eq!(&component_data, first_component_data)
    }

    #[test]
    fn iterate_inserted_components() {
        let mut world = World::new();
        let component_datas = vec![
            &TestComponent(123),
            &TestComponent(456),
            &TestComponent(789),
        ];
        let component_datas_copy = component_datas.clone();
        for component_data in component_datas.into_iter().cloned() {
            let entity = world.new_entity();
            world.add_component_to_entity(entity, component_data);
        }

        let actual_component_datas: Vec<&TestComponent> = world.iterate().collect();

        assert_eq!(component_datas_copy, actual_component_datas)
    }
}
