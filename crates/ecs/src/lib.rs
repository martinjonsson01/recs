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

use std::any::TypeId;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{any, fmt};

use crate::scheduling::{Schedule, ScheduleExecutor};
use crossbeam::channel::Receiver;
use rayon::prelude::*;

use crate::storage::{ComponentVec, ComponentVecImpl};

pub mod pool;
pub mod scheduler_rayon;
pub mod scheduling;
mod storage;
mod system_precedence;

/// A container for the `ecs::System`s that run in the application.
#[derive(Debug, Default)]
pub struct Application {
    world: World,
    pub systems: Vec<Box<dyn System>>,
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

    pub fn add_systems<F: IntoSystem<Parameters>, Parameters: SystemParameter>(
        mut self,
        functions: impl IntoIterator<Item = F>,
    ) -> Self {
        for function in functions {
            self.systems.push(Box::new(function.into_system()));
        }
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
}

impl Application {
    pub fn run<'a, Executor, S>(&'a mut self, shutdown_receiver: Receiver<()>)
    where
        Executor: ScheduleExecutor<'a> + Default,
        S: Schedule<'a>,
    {
        let mut executor = Executor::default();
        let schedule = S::generate(&self.systems);
        executor.execute(schedule, &self.world, shutdown_receiver)
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
    ) -> Option<RwLockReadGuard<Vec<Option<ComponentType>>>> {
        for component_vec in self.component_vecs.iter() {
            if let Some(component_vec) = component_vec
                .as_any()
                .downcast_ref::<ComponentVecImpl<ComponentType>>()
            {
                return Some(component_vec.read().expect("poisoned lock"));
            }
        }
        None
    }

    #[allow(dead_code)]
    fn borrow_component_vec_mut<ComponentType: 'static>(
        &self,
    ) -> Option<RwLockWriteGuard<Vec<Option<ComponentType>>>> {
        for component_vec in self.component_vecs.iter() {
            if let Some(component_vec) = component_vec
                .as_any()
                .downcast_ref::<ComponentVecImpl<ComponentType>>()
            {
                return Some(component_vec.write().expect("poisoned lock"));
            }
        }
        None
    }
}

#[derive(Debug)]
pub struct Read<'a, T: Send + Sync> {
    pub output: &'a T,
}

#[derive(Debug)]
pub struct Write<'a, T: Send + Sync> {
    pub output: &'a mut T,
}

pub trait System: Send + Sync + Debug + Display {
    fn run(&self, world: &World);
    fn run_concurrent(&self, world: &World);
    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor>;
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
pub enum ComponentAccessDescriptor {
    Read(TypeId),
    Write(TypeId),
}

impl ComponentAccessDescriptor {
    fn read<ComponentType: 'static>() -> Self {
        let type_id = TypeId::of::<ComponentType>();
        Self::Read(type_id)
    }
    fn write<ComponentType: 'static>() -> Self {
        let type_id = TypeId::of::<ComponentType>();
        Self::Write(type_id)
    }

    pub fn component_type(&self) -> TypeId {
        match &self {
            ComponentAccessDescriptor::Read(component_type)
            | ComponentAccessDescriptor::Write(component_type) => *component_type,
        }
    }

    pub fn is_write(&self) -> bool {
        match &self {
            ComponentAccessDescriptor::Read(_) => false,
            ComponentAccessDescriptor::Write(_) => true,
        }
    }

    pub fn is_read(&self) -> bool {
        !self.is_write()
    }
}

pub trait IntoSystem<Parameters> {
    type Output: System + 'static;

    fn into_system(self) -> Self::Output;
}

pub struct FunctionSystem<F, Parameters: SystemParameter> {
    system: F,
    parameters: PhantomData<Parameters>,
}

impl<F, Parameters: SystemParameter> Display for FunctionSystem<F, Parameters> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let function_name = any::type_name::<F>();
        let system_name = if let Some(colon_index) = function_name.rfind("::") {
            &function_name[colon_index + 2..]
        } else {
            function_name
        };
        writeln!(f, "{system_name}")
    }
}

impl<F, Parameters: SystemParameter> Debug for FunctionSystem<F, Parameters> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let function_name = any::type_name::<F>();
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

impl<F: Send + Sync, Parameters: SystemParameter> System for FunctionSystem<F, Parameters>
where
    F: SystemParameterFunction<Parameters> + 'static,
{
    fn run(&self, world: &World) {
        SystemParameterFunction::run(&self.system, world);
    }

    fn run_concurrent(&self, world: &World) {
        SystemParameterFunction::run_concurrent(&self.system, world);
    }

    fn component_accesses(&self) -> Vec<ComponentAccessDescriptor> {
        Parameters::component_accesses()
    }
}

impl<F: Send + Sync, Parameters: SystemParameter + 'static> IntoSystem<Parameters> for F
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

// todo: implement with macro
pub trait SystemParameter: Send + Sync {
    fn component_accesses() -> Vec<ComponentAccessDescriptor>;
}

impl<'a, T: Send + Sync + 'static> SystemParameter for Read<'a, T> {
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![ComponentAccessDescriptor::read::<T>()]
    }
}
impl<'a, T: Send + Sync + 'static> SystemParameter for Write<'a, T> {
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![ComponentAccessDescriptor::write::<T>()]
    }
}
impl SystemParameter for () {
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![]
    }
}
impl<P0: SystemParameter> SystemParameter for (P0,) {
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        P0::component_accesses()
    }
}
impl<'a, P0: Send + Sync + 'static, P1: Send + Sync + 'static> SystemParameter
    for (Read<'a, P0>, Read<'a, P1>)
{
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![
            ComponentAccessDescriptor::read::<P0>(),
            ComponentAccessDescriptor::read::<P1>(),
        ]
    }
}
impl<'a, P0: Send + Sync + 'static, P1: Send + Sync + 'static> SystemParameter
    for (Write<'a, P0>, Write<'a, P1>)
{
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![
            ComponentAccessDescriptor::write::<P0>(),
            ComponentAccessDescriptor::write::<P1>(),
        ]
    }
}
impl<'a, P0: Send + Sync + 'static, P1: Send + Sync + 'static> SystemParameter
    for (Read<'a, P0>, Write<'a, P1>)
{
    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![
            ComponentAccessDescriptor::read::<P0>(),
            ComponentAccessDescriptor::write::<P1>(),
        ]
    }
}

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
//TODO: Benchmark run_concurrent implementation against differing chunk sizes.
impl<'a, F, P0: Send + Sync + 'static> SystemParameterFunction<(Read<'a, P0>,)> for F
where
    F: Fn(Read<P0>) + Send + Sync + 'static,
{
    fn run(&self, world: &World) {
        println!("running system with parameter");

        let component_vec = world.borrow_component_vec::<P0>();
        if let Some(components) = component_vec {
            for component in components.iter().filter_map(|c| c.as_ref()) {
                self(Read { output: component })
            }
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                any::type_name::<P0>()
            );
        }
    }
    fn run_concurrent(&self, world: &World) {
        println!("running system with parameter");

        let component_vec = world.borrow_component_vec::<P0>();
        if let Some(components) = component_vec {
            components
                .par_iter()
                .filter_map(|c| c.as_ref())
                .for_each(|component| self(Read { output: component }))
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                any::type_name::<P0>()
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

        let component_vec = world.borrow_component_vec_mut::<P0>();
        if let Some(mut components) = component_vec {
            for component in components.iter_mut().filter_map(|c| c.as_mut()) {
                self(Write { output: component })
            }
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                any::type_name::<P0>()
            );
        }
    }

    fn run_concurrent(&self, world: &World) {
        println!("running system with mutable parameter");

        let component_vec = world.borrow_component_vec_mut::<P0>();
        if let Some(mut components) = component_vec {
            components
                .par_iter_mut()
                .filter_map(|c| c.as_mut())
                .for_each(|component| self(Write { output: component }));
        } else {
            eprintln!(
                "failed to find component vec of type {:?}",
                any::type_name::<P0>()
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
            let components = components0.iter().zip(components1.iter());
            for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_ref()?))) {
                self(Read { output: c0 }, Read { output: c1 })
            }
        } else {
            let type0_name = any::type_name::<P0>();
            let type1_name = any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }

    fn run_concurrent(&self, world: &World) {
        println!("running system with two parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec::<P1>();
        if let (Some(components0), Some(components1)) = (component0_vec, component1_vec) {
            let components = components0.iter().zip(components1.iter());

            components
                .filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_ref()?)))
                .collect::<Vec<_>>()
                .par_iter()
                .for_each(|(c0, c1)| self(Read { output: c0 }, Read { output: c1 }));
        } else {
            let type0_name = any::type_name::<P0>();
            let type1_name = any::type_name::<P1>();
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

        let component0_vec = world.borrow_component_vec_mut::<P0>();
        let component1_vec = world.borrow_component_vec_mut::<P1>();
        if let (Some(mut components0), Some(mut components1)) = (component0_vec, component1_vec) {
            let components = components0.iter_mut().zip(components1.iter_mut());
            for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_mut()?, c1.as_mut()?))) {
                self(Write { output: c0 }, Write { output: c1 })
            }
        } else {
            let type0_name = any::type_name::<P0>();
            let type1_name = any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }

    fn run_concurrent(&self, world: &World) {
        println!("running system with two mutable parameters");

        let component0_vec = world.borrow_component_vec_mut::<P0>();
        let component1_vec = world.borrow_component_vec_mut::<P1>();
        if let (Some(mut components0), Some(mut components1)) = (component0_vec, component1_vec) {
            let components = components0.iter_mut().zip(components1.iter_mut());

            components
                .filter_map(|(c0, c1)| Some((c0.as_mut()?, c1.as_mut()?)))
                .collect::<Vec<_>>()
                .par_iter_mut()
                .for_each(|(c0, c1)| self(Write { output: c0 }, Write { output: c1 }));
        } else {
            let type0_name = any::type_name::<P0>();
            let type1_name = any::type_name::<P1>();
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
        let component1_vec = world.borrow_component_vec_mut::<P1>();
        if let (Some(components0), Some(mut components1)) = (component0_vec, component1_vec) {
            let components = components0.iter().zip(components1.iter_mut());
            for (c0, c1) in components.filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_mut()?))) {
                self(Read { output: c0 }, Write { output: c1 })
            }
        } else {
            let type0_name = any::type_name::<P0>();
            let type1_name = any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }

    fn run_concurrent(&self, world: &World) {
        println!("running system with two mutable parameters");

        let component0_vec = world.borrow_component_vec::<P0>();
        let component1_vec = world.borrow_component_vec_mut::<P1>();
        if let (Some(components0), Some(mut components1)) = (component0_vec, component1_vec) {
            let components = components0.iter().zip(components1.iter_mut());

            components
                .filter_map(|(c0, c1)| Some((c0.as_ref()?, c1.as_mut()?)))
                .collect::<Vec<_>>()
                .par_iter_mut()
                .for_each(|(c0, c1)| self(Read { output: c0 }, Write { output: c1 }));
        } else {
            let type0_name = any::type_name::<P0>();
            let type1_name = any::type_name::<P1>();
            eprintln!("failed to find component vec of type <{type0_name}, {type1_name}>");
        }
    }
}

#[cfg(test)]
mod ecs_tests {
    use std::sync::{Arc, Mutex};

    use crate::scheduling::{Sequential, Unordered};
    use crossbeam::channel::bounded;

    use super::*;

    #[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
    struct TestComponent(i32);

    #[test]
    fn iterate_inserted_component() {
        let mut application: Application = Application::default();
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
        let mut application: Application = Application::default();
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
        let mut application: Application = Application::default();
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

    #[test]
    fn system_read_component() {
        let mut application: Application = Application::default();
        let entity = application.new_entity();
        let component_data = TestComponent(218);
        application.add_component_to_entity(entity, component_data);

        let system =
            move |component: Read<TestComponent>| assert_eq!(&component_data, component.output);
        let (_, shutdown_receiver) = bounded(1);
        application
            .add_system(system)
            .run::<Sequential, Unordered>(shutdown_receiver);
    }

    #[test]
    fn system_read_components() {
        let mut application: Application = Application::default();
        let component_datas = vec![TestComponent(123), TestComponent(456), TestComponent(789)];
        let component_count = component_datas.len();
        for component_data in component_datas.iter().cloned() {
            let entity = application.new_entity();
            application.add_component_to_entity(entity, component_data);
        }

        let (shutdown_sender, shutdown_receiver) = bounded(1);

        let read_components_ref = Arc::new(Mutex::new(vec![]));
        let read_components = Arc::clone(&read_components_ref);
        let system = move |component: Read<TestComponent>| {
            let mut read_components = read_components.try_lock().unwrap();
            read_components.push(*component.output);
            if read_components.len() == component_count {
                shutdown_sender.send(()).unwrap();
            }
        };
        application
            .add_system(system)
            .run::<Sequential, Unordered>(shutdown_receiver);

        assert_eq!(component_datas, *read_components_ref.try_lock().unwrap());
    }

    #[test]
    fn system_mutates_components_other_system_reads_mutated_values() {
        let mut application: Application = Application::default();
        let component_datas = vec![TestComponent(123), TestComponent(456), TestComponent(789)];
        for component_data in component_datas.iter().cloned() {
            let entity = application.new_entity();
            application.add_component_to_entity(entity, component_data);
        }

        let mutator_system = |component: Write<TestComponent>| {
            *component.output = TestComponent(0);
        };

        let (shutdown_sender, shutdown_receiver) = bounded(1);

        let read_components_ref = Arc::new(Mutex::new(vec![]));
        let read_components = Arc::clone(&read_components_ref);
        let read_system = move |component: Read<TestComponent>| {
            let mut read_components = read_components.try_lock().unwrap();
            read_components.push(*component.output);
            if read_components.len() == component_datas.len() {
                shutdown_sender.send(()).unwrap();
            }
        };

        application
            .add_system(mutator_system)
            .add_system(read_system)
            .run::<Sequential, Unordered>(shutdown_receiver);

        assert_eq!(
            vec![TestComponent(0), TestComponent(0), TestComponent(0)],
            *read_components_ref.try_lock().unwrap()
        );
    }
}
