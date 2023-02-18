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
use std::sync::RwLock;
use rayon::prelude::*;

use crate::storage::{ComponentVec, ComponentVecImpl};

mod storage;

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

    pub fn add_component_to_entity<ComponentType: Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) {
        for component_vec in self.world.component_vecs.iter_mut() {
            if let Some(component_vec) = component_vec
                .as_any_mut()
                .downcast_mut::<ComponentVecImpl<ComponentType>>()
            {
                component_vec.write().unwrap()[entity.0] = Some(component);
                return;
            }
        }

        self.world.create_component_vec_and_add(entity, component);
    }

    pub fn run2(&mut self) {
        for system in &mut self.systems {
            system.run(&self.world);
        }
    }

    pub fn run(&mut self){
        let systems = &mut self.systems;
        let world = &mut self.world;
        let _ = &mut systems.par_iter_mut().for_each(|p: &mut Box<dyn System>| p.run(world));
    }
}

impl World {
    fn create_component_vec_and_add<ComponentType: Send + Sync + 'static>(
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
            .push(Box::new(RwLock::new(new_component_vec)))
    }

    #[allow(dead_code)]
    fn borrow_component_vec<ComponentType: Send + Sync + 'static>(
        &self,
    ) -> Option<&RwLock<Vec<Option<ComponentType>>>> {
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

    /*#[allow(dead_code)]
    fn borrow_component_vec_mut<ComponentType: 'static>(
        &self,
    ) -> Option<Vec<Option<ComponentType>>> {
        for component_vec in self.component_vecs.iter() {
            if let Some(component_vec) = component_vec
                .as_any()
                .downcast_ref::<ComponentVecImpl<ComponentType>>()
            {
                return Some(*component_vec.get_mut().unwrap());
            }
        }
        None
    }*/
}

#[derive(Debug)]
pub struct Read<'a, T> {
    pub output: &'a T,
}

#[derive(Debug)]
pub struct Write<'a, T> {
    pub output: &'a mut T,
}

pub trait System: Send + Sync {
    fn run(&self, world: &World);
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

impl<F, Parameters: SystemParameter + Send + Sync> System for FunctionSystem<F, Parameters>
where
    F: SystemParameterFunction<Parameters> + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        SystemParameterFunction::run(&self.system, world);
    }
}

impl<F, Parameters: SystemParameter + Send + Sync + 'static> IntoSystem<Parameters> for F
where
    F: SystemParameterFunction<Parameters> + 'static + Send + Sync,
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

impl<'a, T> SystemParameter for Read<'a, T> {}
impl<'a, T> SystemParameter for Write<'a, T> {}
impl SystemParameter for () {}
impl<P0: SystemParameter> SystemParameter for (P0,) {}
impl<'a, P0, P1> SystemParameter for (Read<'a, P0>, Read<'a, P1>) {}
impl<'a, P0, P1> SystemParameter for (Write<'a, P0>, Write<'a, P1>) {}
impl<'a, P0, P1> SystemParameter for (Read<'a, P0>, Write<'a, P1>) {}

trait SystemParameterFunction<Parameters: SystemParameter>: 'static {
    fn run(&self, world: &World);
}

impl<F> SystemParameterFunction<()> for F
where
    F: Fn() + 'static,
{
    fn run(&self, _world: &World) {
        println!("running system with no parameters");
        self();
    }
}

impl<'a, F, P0: Send + Sync + 'static> SystemParameterFunction<(Read<'a, P0>,)> for F
where
    F: Fn(Read<P0>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with parameter");
        //let mut func = self;
        let component_vec = world.borrow_component_vec::<P0>();
        if let Some(components) = component_vec {
            components.read().unwrap().par_iter().filter_map(|c| c.as_ref()).for_each(|c| self(Read { output: c }));


            /*for component in components.read().unwrap().iter().filter_map(|c| c.as_ref()) {
                self(Read { output: component })
            }*/
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                std::any::type_name::<P0>()
            );
        }
    }
}

impl<'a, F, P0: Send + Sync +'static> SystemParameterFunction<(Write<'a, P0>,)> for F
where
    F: Fn(Write<P0>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with mutable parameter");

        let component_vec = world.borrow_component_vec::<P0>();
        if let Some(components) = component_vec {
            components.write().unwrap().par_iter_mut().filter_map(|c| c.as_mut()).for_each(|c| self(Write { output: c }));
            /*for component in components.write().unwrap().iter_mut().filter_map(|c| c.as_mut()) {
                self(Write { output: component })
            }*/
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                std::any::type_name::<P0>()
            );
        }
    }
}

impl<'a, F, P0: Send + Sync +'static, P1: Send + Sync +'static> SystemParameterFunction<(Read<'a, P0>, Read<'a, P1>)> for F
where
    F: Fn(Read<P0>, Read<P1>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with two parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let componentsL0 = components0.read().unwrap();
            let componentsL1 = components1.read().unwrap();
            let components = componentsL0.par_iter().zip(componentsL1.par_iter());
            components.filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_ref()?))).for_each(|(c0, c1)| self(Read { output: c0 }, Read { output: c1 }));

            /*for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_ref()?))) {
                self(Read { output: c0 }, Read { output: c1 })
            }*/
        } else {
            let type0_name = std::any::type_name::<P0>();
            let type1_name = std::any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }
}

impl<'a, F, P0: Send + Sync +'static, P1: Send + Sync +'static> SystemParameterFunction<(Write<'a, P0>, Write<'a, P1>)> for F
where
    F: Fn(Write<P0>, Write<P1>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with two mutable parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let mut componentsL0 = components0.write().unwrap();
            let mut componentsL1 = components1.write().unwrap();
            let components = componentsL0.par_iter_mut().zip(componentsL1.par_iter_mut());
            components.filter_map(|(c0, c1)| Some((c0.as_mut()?, c1.as_mut()?))).for_each(|(c0, c1)| self(Write { output: c0 }, Write { output: c1 }));
            /*for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_mut()?, c1.as_mut()?))) {
                self(Write { output: c0 }, Write { output: c1 })
            }*/
        } else {
            let type0_name = std::any::type_name::<P0>();
            let type1_name = std::any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }
}

impl<'a, F, P0: Send + Sync +'static, P1: Send + Sync +'static> SystemParameterFunction<(Read<'a, P0>, Write<'a, P1>)> for F
where
    F: Fn(Read<P0>, Write<P1>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with two mutable parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let componentsL0 = components0.read().unwrap();
            let mut componentsL1 = components1.write().unwrap();
            let components = componentsL0.par_iter().zip(componentsL1.par_iter_mut());
            components.filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_mut()?))).for_each(|(c0, c1)| self(Read { output: c0 }, Write { output: c1 }));
            /*for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_mut()?))) {
                self(Read { output: c0 }, Write { output: c1 })
            }*/
        } else {
            let type0_name = std::any::type_name::<P0>();
            let type1_name = std::any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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
        let c = component_vec.write().unwrap();
        let mut components = c.iter().filter_map(|c| c.as_ref());
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
        let c = component_vec.write().unwrap();
        let actual_component_datas: Vec<_> =
            c.iter().filter_map(|c| c.as_ref()).collect();

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

        let components = application
            .world
            .borrow_component_vec::<TestComponent>()
            .unwrap();
        let mut c = components.write().unwrap();
        let components_iter = c
            .iter_mut()
            .filter_map(|component| component.as_mut());
        for component in components_iter {
            *component = TestComponent(1);
        }
        drop(c);

        let temp = application.world.borrow_component_vec().unwrap().read().unwrap();
        let mutated_components: Vec<&TestComponent> = temp
            .iter()
            .filter_map(|component| component.as_ref())
            .collect();
        assert_eq!(
            vec![&TestComponent(1), &TestComponent(1), &TestComponent(1)],
            mutated_components
        )
    }

    #[test]
    fn system_read_component() {
        let mut application = Application::default();
        let entity = application.new_entity();
        let component_data = TestComponent(218);
        application.add_component_to_entity(entity, component_data);

        let system =
            move |component: Read<TestComponent>| assert_eq!(&component_data, component.output);
        application.add_system(system).run();
    }

    #[test]
    fn system_read_components() {
        let mut application = Application::default();
        let component_datas = vec![TestComponent(123), TestComponent(456), TestComponent(789)];
        for component_data in component_datas.iter().cloned() {
            let entity = application.new_entity();
            application.add_component_to_entity(entity, component_data);
        }

        let read_components_ref = Arc::new(RwLock::new(vec![]));
        let read_components = Arc::clone(&read_components_ref);
        let system = move |component: Read<TestComponent>| {
            read_components.write().unwrap().push(*component.output);
        };
        application.add_system(system).run();

        assert_eq!(component_datas, *read_components_ref.read().unwrap());
    }

    #[test]
    fn system_mutates_components_other_system_reads_mutated_values() {
        let mut application = Application::default();
        let component_datas = vec![TestComponent(123), TestComponent(456), TestComponent(789)];
        for component_data in component_datas.iter().cloned() {
            let entity = application.new_entity();
            application.add_component_to_entity(entity, component_data);
        }

        let mutator_system = |component: Write<TestComponent>| {
            *component.output = TestComponent(0);
        };

        let read_components_ref = Arc::new(RwLock::new(vec![]));
        let read_components = Arc::clone(&read_components_ref);
        let read_system = move |component: Read<TestComponent>| {
            read_components.write().unwrap().push(*component.output);
        };

        application
            .add_system(mutator_system)
            .add_system(read_system)
            .run();

        assert_eq!(
            vec![TestComponent(0), TestComponent(0), TestComponent(0)],
            *read_components_ref.read().unwrap()
        );
    }
}
