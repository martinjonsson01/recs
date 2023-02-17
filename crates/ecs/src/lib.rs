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
use std::cell::{Ref, RefCell, RefMut};
use std::fmt::Formatter;
use std::marker::PhantomData;

impl std::fmt::Debug for dyn System + 'static {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "system")
    }
}

/// A container for the `ecs::System`s that run in the application.
#[derive(Debug, Default)]
pub struct Application {
    world: World,
    systems: Vec<Box<dyn System>>,
}

#[derive(Debug, Default)]
pub struct World {
    entity_count: usize,
    component_vecs: Vec<Box<dyn ComponentVec>>,
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Entity(usize);

impl Application {
    pub fn add_system<F: IntoSystem<Parameters>, Parameters: SystemParameter>(
        mut self,
        function: F,
    ) -> Self {
        self.systems.push(Box::new(function.into_system()));
        self
    }

    pub fn new_entity(&mut self) -> Entity {
        let entity_id = self.world.entity_count;
        for component_vec in self.world.component_vecs.iter_mut() {
            component_vec.push_none();
        }
        self.world.entity_count += 1;
        Entity(entity_id)
    }

    pub fn add_component_to_entity<ComponentType: 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        for component_vec in self.world.component_vecs.iter_mut() {
            if let Some(component_vec) = component_vec
                .as_any_mut()
                .downcast_mut::<ComponentVecImpl<ComponentType>>()
            {
                component_vec.get_mut()[entity.0] = Some(component);
                return;
            }
        }

        self.world.create_component_vec_and_add(entity, component);
    }

    pub fn run(&mut self) {
        for system in &mut self.systems {
            system.run(&self.world);
        }
    }
}

impl World {
    fn create_component_vec_and_add<ComponentType: 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        let mut new_component_vec = Vec::with_capacity(self.entity_count);

        for _ in 0..self.entity_count {
            new_component_vec.push(None)
        }

        new_component_vec[entity.0] = Some(component);
        self.component_vecs
            .push(Box::new(RefCell::new(new_component_vec)))
    }

    #[allow(dead_code)]
    fn borrow_component_vec<ComponentType: 'static>(
        &self,
    ) -> Option<Ref<Vec<Option<ComponentType>>>> {
        for component_vec in self.component_vecs.iter() {
            if let Some(component_vec) = component_vec
                .as_any()
                .downcast_ref::<ComponentVecImpl<ComponentType>>()
            {
                return Some(component_vec.borrow());
            }
        }
        None
    }

    #[allow(dead_code)]
    fn borrow_component_vec_mut<ComponentType: 'static>(
        &self,
    ) -> Option<RefMut<Vec<Option<ComponentType>>>> {
        for component_vec in self.component_vecs.iter() {
            if let Some(component_vec) = component_vec
                .as_any()
                .downcast_ref::<ComponentVecImpl<ComponentType>>()
            {
                return Some(component_vec.borrow_mut());
            }
        }
        None
    }
}

#[derive(Debug)]
pub struct Query<'a, T> {
    pub output: &'a T,
}

pub trait System {
    fn run(&mut self, world: &World);
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
    fn run(&mut self, world: &World) {
        SystemParameterFunction::run(&mut self.system, world);
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

impl<'a, T> SystemParameter for Query<'a, T> {}
impl SystemParameter for () {}
impl<P0: SystemParameter> SystemParameter for (P0,) {}
impl<P0: SystemParameter, P1: SystemParameter> SystemParameter for (P0, P1) {}

trait SystemParameterFunction<Parameters: SystemParameter>: 'static {
    fn run(&mut self, world: &World);
}

impl<F> SystemParameterFunction<()> for F
where
    F: Fn() + 'static,
{
    fn run(&mut self, _world: &World) {
        println!("running system with no parameters");
        self();
    }
}

impl<'a, F, ComponentType: 'static> SystemParameterFunction<(Query<'a, ComponentType>,)> for F
where
    F: Fn(Query<ComponentType>) + 'static,
{
    fn run(&mut self, world: &World) {
        eprintln!("running system with parameter");

        let component_vec = world.borrow_component_vec::<ComponentType>();
        if let Some(components) = component_vec {
            for component in components.iter().filter_map(|c| c.as_ref()) {
                self(Query { output: component })
            }
        }
    }
}

impl<F, P0: SystemParameter, P1: SystemParameter> SystemParameterFunction<(P0, P1)> for F
where
    F: Fn(P0, P1) + 'static,
{
    fn run(&mut self, _world: &World) {
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
        let mut application = Application::default();
        let entity = application.new_entity();
        let component_data = TestComponent(218);
        application.add_component_to_entity(entity, component_data);

        let component_vec = application.world.borrow_component_vec().unwrap();
        let mut components = component_vec.iter().filter_map(|c| c.as_ref());
        let first_component_data = components.next().unwrap();

        assert_eq!(&component_data, first_component_data)
    }

    #[test]
    fn iterate_inserted_components() {
        let mut application = Application::default();
        let component_datas = vec![
            &TestComponent(123),
            &TestComponent(456),
            &TestComponent(789),
        ];
        let component_datas_copy = component_datas.clone();
        for component_data in component_datas.into_iter().cloned() {
            let entity = application.new_entity();
            application.add_component_to_entity(entity, component_data);
        }

        let component_vec = application.world.borrow_component_vec().unwrap();
        let actual_component_datas: Vec<_> =
            component_vec.iter().filter_map(|c| c.as_ref()).collect();

        assert_eq!(component_datas_copy, actual_component_datas)
    }

    #[test]
    fn mutate_components() {
        let mut application = Application::default();
        let component_datas = vec![
            &TestComponent(123),
            &TestComponent(456),
            &TestComponent(789),
        ];
        for component_data in component_datas.into_iter().cloned() {
            let entity = application.new_entity();
            application.add_component_to_entity(entity, component_data);
        }

        let mut components = application
            .world
            .borrow_component_vec_mut::<TestComponent>()
            .unwrap();
        let components_iter = components
            .iter_mut()
            .filter_map(|component| component.as_mut());
        for component in components_iter {
            *component = TestComponent(1);
        }
        drop(components);

        let temp = application.world.borrow_component_vec().unwrap();
        let mutated_components: Vec<&TestComponent> = temp
            .iter()
            .filter_map(|component| component.as_ref())
            .collect();
        assert_eq!(
            vec![&TestComponent(1), &TestComponent(1), &TestComponent(1)],
            mutated_components
        )
    }
}
