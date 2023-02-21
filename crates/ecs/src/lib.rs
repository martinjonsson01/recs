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

use rayon::prelude::*;
use std::fmt::Formatter;
use std::marker::PhantomData;
use std::sync::RwLock;

use crate::storage::{ComponentVec, ComponentVecImpl};

mod pool;
mod storage;

impl std::fmt::Debug for dyn System + 'static {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "system")
    }
}

pub trait Schedule: std::fmt::Debug {
    fn execute(&self, systems: &mut Vec<Box<dyn System>>, world: &World);
}

#[derive(Debug, Default)]
pub struct Sequential;

impl Schedule for Sequential {
    fn execute(&self, systems: &mut Vec<Box<dyn System>>, world: &World) {
        for system in systems {
            system.run(world);
        }
    }
}

/// Iterative parallel execution of systems using rayon.
/// Unordered schedule and no safeguards against race conditions.
#[derive(Debug, Default)]
pub struct RayonChaos;

impl Schedule for RayonChaos {
    fn execute(&self, systems: &mut Vec<Box<dyn System>>, world: &World) {
        systems
            .par_iter()
            .for_each(|system| system.run_concurrent(world));
        println!("Concurrent says Hello");
    }
}
/// A container for the `ecs::System`s that run in the application.
#[derive(Debug, Default)]
pub struct Application<Scheduling: Schedule> {
    world: World,
    systems: Vec<Box<dyn System>>,
    schedule: Box<Scheduling>,
}

#[derive(Debug, Default)]
pub struct World {
    entity_count: usize,
    component_vecs: Vec<Box<dyn ComponentVec>>,
}

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Entity(usize);

impl<Scheduling: Schedule> Application<Scheduling> {
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
                component_vec.write().expect("Lock is poisoned")[entity.0] = Some(component);
                return;
            }
        }

        self.world.create_component_vec_and_add(entity, component);
    }

    pub fn run(&mut self) {
        self.schedule.execute(&mut self.systems, &self.world)
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
    fn borrow_component_vec<ComponentType: 'static>(
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
    fn run_concurrent(&self, world: &World);
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
    fn run(&self, world: &World) {
        SystemParameterFunction::run(&self.system, world);
    }

    fn run_concurrent(&self, world: &World) {
        SystemParameterFunction::run_concurrent(&self.system, world);
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

pub trait SystemParameter: Send + Sync {}

impl<'a, T: Send + Sync> SystemParameter for Read<'a, T> {}
impl<'a, T: Send + Sync> SystemParameter for Write<'a, T> {}
impl SystemParameter for () {}
impl<P0: SystemParameter> SystemParameter for (P0,) {}
impl<'a, P0: Send + Sync, P1: Send + Sync> SystemParameter for (Read<'a, P0>, Read<'a, P1>) {}
impl<'a, P0: Send + Sync, P1: Send + Sync> SystemParameter for (Write<'a, P0>, Write<'a, P1>) {}
impl<'a, P0: Send + Sync, P1: Send + Sync> SystemParameter for (Read<'a, P0>, Write<'a, P1>) {}

trait SystemParameterFunction<Parameters: SystemParameter>: Send + Sync + 'static {
    fn run(&self, world: &World);
    fn run_concurrent(&self, world: &World);
}

impl<F> SystemParameterFunction<()> for F
where
    F: Fn() + Send + Sync + 'static,
{
    fn run(&self, _world: &World) {
        println!("running system with no parameters");
        self();
    }
    fn run_concurrent(&self, world: &World) {
        self.run(world);
    }
}

impl<'a, F, P0: Send + Sync + 'static> SystemParameterFunction<(Read<'a, P0>,)> for F
where
    F: Fn(Read<P0>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with parameter");

        let component_vec = world.borrow_component_vec::<P0>();
        if let Some(components) = component_vec {
            for component in components
                .read()
                .expect("Lock is poisoned")
                .iter()
                .filter_map(|c| c.as_ref())
            {
                self(Read { output: component })
            }
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                std::any::type_name::<P0>()
            );
        }
    }
    fn run_concurrent(&self, world: &World) {
        println!("running system with parameter");

        let component_vec = world.borrow_component_vec::<P0>();
        if let Some(components) = component_vec {
            let components = components.read().expect("Lock is poisoned");
            components
                .par_iter()
                .filter_map(|c| c.as_ref())
                .for_each(|component| self(Read { output: component }))
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                std::any::type_name::<P0>()
            );
        }
    }
}

impl<'a, F, P0: Send + Sync + 'static> SystemParameterFunction<(Write<'a, P0>,)> for F
where
    F: Fn(Write<P0>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with mutable parameter");

        let component_vec = world.borrow_component_vec::<P0>();
        if let Some(components) = component_vec {
            for component in components
                .write()
                .expect("Lock is poisoned")
                .iter_mut()
                .filter_map(|c| c.as_mut())
            {
                self(Write { output: component })
            }
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                std::any::type_name::<P0>()
            );
        }
    }

    fn run_concurrent(&self, world: &World) {
        println!("running system with mutable parameter");

        let component_vec = world.borrow_component_vec::<P0>();
        if let Some(components) = component_vec {
            let mut components = components.write().expect("Lock is poisoned");
            components
                .par_iter_mut()
                .filter_map(|c| c.as_mut())
                .for_each(|component| self(Write { output: component }));
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                std::any::type_name::<P0>()
            );
        }
    }
}

impl<'a, F, P0: Send + Sync + 'static, P1: Send + Sync + 'static>
    SystemParameterFunction<(Read<'a, P0>, Read<'a, P1>)> for F
where
    F: Fn(Read<P0>, Read<P1>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with two parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let components0 = components0.read().expect("Lock is poisoned");
            let components1 = components1.read().expect("Lock is poisoned");
            let components = components0.iter().zip(components1.iter());
            for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_ref()?))) {
                self(Read { output: c0 }, Read { output: c1 })
            }
        } else {
            let type0_name = std::any::type_name::<P0>();
            let type1_name = std::any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }

    fn run_concurrent(&self, world: &World) {
        println!("running system with two parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let components0 = components0.read().expect("Lock is poisoned");
            let components1 = components1.read().expect("Lock is poisoned");
            let components = components0.iter().zip(components1.iter());

            components
                .filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_ref()?)))
                .collect::<Vec<_>>()
                .par_iter()
                .for_each(|(c0, c1)| self(Read { output: c0 }, Read { output: c1 }));
        } else {
            let type0_name = std::any::type_name::<P0>();
            let type1_name = std::any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }
}

impl<'a, F, P0: Send + Sync + 'static, P1: Send + Sync + 'static>
    SystemParameterFunction<(Write<'a, P0>, Write<'a, P1>)> for F
where
    F: Fn(Write<P0>, Write<P1>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with two mutable parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let mut components0 = components0.write().expect("Lock is poisoned");
            let mut components1 = components1.write().expect("Lock is poisoned");

            let components = components0.iter_mut().zip(components1.iter_mut());
            for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_mut()?, c1.as_mut()?))) {
                self(Write { output: c0 }, Write { output: c1 })
            }
        } else {
            let type0_name = std::any::type_name::<P0>();
            let type1_name = std::any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }

    fn run_concurrent(&self, world: &World) {
        println!("running system with two mutable parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let mut components0_locked = components0.write().expect("Lock is poisoned");
            let mut components1_locked = components1.write().expect("Lock is poisoned");

            let components = components0_locked
                .iter_mut()
                .zip(components1_locked.iter_mut());

            components
                .filter_map(|(c0, c1)| Some((c0.as_mut()?, c1.as_mut()?)))
                .collect::<Vec<_>>()
                .par_iter_mut()
                .for_each(|(c0, c1)| self(Write { output: c0 }, Write { output: c1 }));
        } else {
            let type0_name = std::any::type_name::<P0>();
            let type1_name = std::any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }
}

impl<'a, F, P0: Send + Sync + 'static, P1: Send + Sync + 'static>
    SystemParameterFunction<(Read<'a, P0>, Write<'a, P1>)> for F
where
    F: Fn(Read<P0>, Write<P1>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with two mutable parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let components0_locked = components0.read().expect("Lock is poisoned");
            let mut components1_locked = components1.write().expect("Lock is poisoned");

            let components = components0_locked.iter().zip(components1_locked.iter_mut());
            for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_mut()?))) {
                self(Read { output: c0 }, Write { output: c1 })
            }
        } else {
            let type0_name = std::any::type_name::<P0>();
            let type1_name = std::any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }

    fn run_concurrent(&self, world: &World) {
        println!("running system with two mutable parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let components0_locked = components0.read().expect("Lock is poisoned");
            let mut components1_locked = components1.write().expect("Lock is poisoned");

            let components = components0_locked.iter().zip(components1_locked.iter_mut());

            components
                .filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_mut()?)))
                .collect::<Vec<_>>()
                .par_iter_mut()
                .for_each(|(c0, c1)| self(Read { output: c0 }, Write { output: c1 }));
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
        let mut application: Application<Sequential> = Application::default();
        let entity = application.new_entity();
        let component_data = TestComponent(218);
        application.add_component_to_entity(entity, component_data);

        let component_vec = application.world.borrow_component_vec().unwrap();
        let component_vec = component_vec.read().expect("Lock is poisoned");
        let mut components = component_vec.iter().filter_map(|c| c.as_ref());
        let first_component_data = components.next().unwrap();

        assert_eq!(&component_data, first_component_data)
    }

    #[test]
    fn iterate_inserted_components() {
        let mut application: Application<Sequential> = Application::default();
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

        let component_vec = application
            .world
            .borrow_component_vec()
            .unwrap()
            .read()
            .expect("Lock is poisoned");
        let actual_component_datas: Vec<_> =
            component_vec.iter().filter_map(|c| c.as_ref()).collect();

        assert_eq!(component_datas_copy, actual_component_datas)
    }

    #[test]
    fn mutate_components() {
        let mut application: Application<Sequential> = Application::default();
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
            .borrow_component_vec::<TestComponent>()
            .unwrap()
            .write()
            .expect("Lock is poisoned");
        let components_iter = components
            .iter_mut()
            .filter_map(|component| component.as_mut());
        for component in components_iter {
            *component = TestComponent(1);
        }
        drop(components);

        let temp = application
            .world
            .borrow_component_vec()
            .unwrap()
            .read()
            .expect("Lock is poisoned");
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
        let mut application: Application<Sequential> = Application::default();
        let entity = application.new_entity();
        let component_data = TestComponent(218);
        application.add_component_to_entity(entity, component_data);

        let system =
            move |component: Read<TestComponent>| assert_eq!(&component_data, component.output);
        application.add_system(system).run();
    }

    #[test]
    fn system_read_components() {
        let mut application: Application<Sequential> = Application::default();
        let component_datas = vec![TestComponent(123), TestComponent(456), TestComponent(789)];
        for component_data in component_datas.iter().cloned() {
            let entity = application.new_entity();
            application.add_component_to_entity(entity, component_data);
        }

        let read_components_ref = Arc::new(RwLock::new(vec![]));
        let read_components = Arc::clone(&read_components_ref);
        let system = move |component: Read<TestComponent>| {
            read_components
                .write()
                .expect("Lock is poisoned")
                .push(*component.output);
        };
        application.add_system(system).run();

        assert_eq!(
            component_datas,
            *read_components_ref.read().expect("Lock is poisoned")
        );
    }

    #[test]
    fn system_mutates_components_other_system_reads_mutated_values() {
        let mut application: Application<Sequential> = Application::default();
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
            read_components
                .write()
                .expect("Lock is poisoned")
                .push(*component.output);
        };

        application
            .add_system(mutator_system)
            .add_system(read_system)
            .run();

        assert_eq!(
            vec![TestComponent(0), TestComponent(0), TestComponent(0)],
            *read_components_ref.read().expect("Lock is poisoned")
        );
    }
}
