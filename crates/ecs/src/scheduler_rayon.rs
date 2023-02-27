use crate::{Schedule, System, SystemParametersVec, World};
use crossbeam::channel::Receiver;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::ops::Deref;

type EnumeratedSystemParametersVec = Vec<(usize, (Vec<Box<str>>, Vec<Box<str>>))>;

#[derive(Debug, Default)]
pub struct DAG {
    pub nodes: Vec<usize>,
    pub edges: Vec<(usize, usize)>,
}

impl DAG {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
    pub fn add_node(&mut self, node: usize) {
        if !self.nodes.contains(&node) {
            self.nodes.push(node);
        }
    }
    pub fn add_edge(&mut self, from: usize, to: usize) {
        if !self.edges.contains(&(from, to)) {
            self.edges.push((from, to));
        }
    }

    pub fn generate_dag(&mut self, parameters: SystemParametersVec) {
        let binding = parameters
            .iter()
            .enumerate()
            .map(|(i, x)| (i, x.clone()))
            .collect();
        let parameters_with_system_index: EnumeratedSystemParametersVec = binding;
        let mut dependencies: BTreeMap<String, Vec<(usize, bool)>> = BTreeMap::new();
        //Add all systems to dependencies btreemap with each component as key
        for current_system in parameters_with_system_index {
            current_system.1 .0.iter().for_each(|component| {
                if let Some(systems_w_dependencies) = dependencies.get_mut(component.deref()) {
                    systems_w_dependencies.push((current_system.0, false));
                } else {
                    dependencies.insert(
                        component.clone().into_string(),
                        vec![(current_system.0, false)],
                    );
                }
            });
            current_system.1 .1.iter().for_each(|component| {
                if let Some(systems_w_dependencies) = dependencies.get_mut(component.deref()) {
                    systems_w_dependencies.push((current_system.0, true));
                } else {
                    dependencies.insert(
                        component.clone().into_string(),
                        vec![(current_system.0, true)],
                    );
                }
            });
        }

        for component_type in dependencies.iter_mut() {
            let mut writes_vec: Vec<(usize, bool)> =
                component_type.1.iter().filter(|x| x.1).copied().collect();
            let systems = component_type.1;

            //Reverse to use pop from front
            writes_vec.reverse();
            systems.reverse();

            let mut prev_write: Option<usize> = None;
            let mut next_write: Option<usize> = None;
            if let Some(write) = writes_vec.pop() {
                next_write = Some(write.0);
            }
            while !systems.is_empty() {
                if let Some(sys) = systems.pop() {
                    self.add_node(sys.0);
                    println!("Added node {}", sys.0);
                    if let Some(write) = next_write {
                        //Handle overlap
                        if sys.0 == write {
                            //Replace next/prev write
                            prev_write = Some(sys.0);
                            if let Some(write) = writes_vec.pop() {
                                next_write = Some(write.0);
                                if let Some(next_sys) = systems.last() {
                                    if next_sys.0 == write.0 {
                                        self.add_edge(sys.0, next_sys.0);
                                    }
                                }
                            }
                            //No writes left
                            else {
                                next_write = None;
                            }
                        }
                        //No overlap handle
                        else {
                            if let Some(index) = prev_write {
                                self.add_edge(index, sys.0);
                            }
                            self.add_edge(sys.0, write);
                        }
                    }
                    //No writes left handle
                    else if let Some(index) = prev_write {
                        self.add_edge(index, sys.0);
                    }
                } else {
                    eprintln!("Tried to pop when systems.len() == 0");
                }
            }
        }
        println!("GENERATED NODES: {:?}", self.nodes);
        println!("GENERATED EDGES: {:?}", self.edges);
    }
}

/// Each stage consists of independent systems.
/// No user assigned ordering is taken into account. This is an avenue of development.
/// Is quite slow for a large amount of systems without optimization and maybe with.
/// Top down approach might be better?
/// In conclusion, this is a very naive implementation and needs review.
pub fn generate_stages(parameters: SystemParametersVec) -> Vec<EnumeratedSystemParametersVec> {
    let binding = parameters
        .iter()
        .enumerate()
        .map(|(i, x)| (i, x.clone()))
        .collect();
    let parameters_with_system_index: EnumeratedSystemParametersVec = binding;
    let mut stages: Vec<EnumeratedSystemParametersVec> = Vec::new();

    for system_current in parameters_with_system_index {
        let mut inserted = false;
        for stage in stages.iter_mut() {
            let mut conflict = false;
            for system in stage.iter() {
                system_current.1 .0.iter().for_each(|component| {
                    if system.1 .1.iter().any(|x| x.deref() == component.deref()) {
                        conflict = true;
                    }
                });
                system_current.1 .1.iter().for_each(|component| {
                    if system.1 .1.iter().any(|x| x.deref() == component.deref()) {
                        conflict = true;
                    };
                });
            }
            if !conflict {
                stage.push(system_current.clone());
                inserted = true;
                break;
            }
        }
        if !inserted {
            stages.push(vec![system_current.clone()]);
        }
    }
    println!("GENERATED STAGES: {stages:?}");
    stages.to_vec()
}

/// Iterative parallel execution of systems using rayon.
/// Unordered schedule and no safeguards against race conditions or deadlocks.
#[derive(Debug, Default)]
pub struct RayonChaos;

impl<'a> Schedule<'a> for RayonChaos {
    fn execute(
        &mut self,
        systems: &'a mut Vec<Box<dyn System>>,
        world: &'a World,
        _parameters: &'a SystemParametersVec,
        _shutdown_receiver: Receiver<()>,
    ) {
        loop {
            systems
                .par_iter()
                .for_each(|system| system.run_concurrent(world));
        }
    }
}

/// Iterative parallel execution of systems using rayon.
/// A stage only contains systems that do not depend on each other.
/// Stages can swap locations with one another if
/// order of when user added systems is not relevant.

#[derive(Debug, Default)]
pub struct RayonStaged;

impl<'a> Schedule<'a> for RayonStaged {
    fn execute(
        &mut self,
        systems: &'a mut Vec<Box<dyn System>>,
        world: &'a World,
        parameters: &'a SystemParametersVec,
        _shutdown_receiver: Receiver<()>,
    ) {
        let stages = generate_stages(parameters.to_vec());
        //Commented for now since it makes testing difficult.
        //loop {
        stages.iter().for_each(|stage| {
            stage
                .par_iter()
                .for_each(|k| systems[k.0].run_concurrent(world));
        });
        //}
    }
}

#[cfg(test)]
mod scheduler_tests {
    use crate::scheduler_rayon::{generate_stages, RayonStaged, DAG};
    use crate::{Application, Read, SystemParametersVec, Write};
    use crossbeam::channel::unbounded;

    #[derive(Debug, Default, Clone, Ord, PartialOrd, Eq, PartialEq, Copy)]
    pub struct TestComponent1(pub u32);

    #[derive(Debug, Default, Clone, Ord, PartialOrd, Eq, PartialEq, Copy)]
    pub struct TestComponent2(pub u32);

    #[derive(Debug, Default, Clone, Ord, PartialOrd, Eq, PartialEq, Copy)]
    pub struct TestComponent3(pub u32);

    fn writing_system(component: Write<TestComponent1>) {
        component.output.0 += 100;
    }

    fn reading_writing_system(component1: Read<TestComponent1>, component2: Write<TestComponent2>) {
        component2.output.0 += component1.output.0 / 2;
    }

    #[test]
    fn rayon_staged_mutation() {
        let mut application: Application = Application::default();
        let entity = application.new_entity();
        let component1_data = TestComponent1(100);
        let component2_data = TestComponent2(200);
        application.add_component_to_entity(entity, component1_data);
        application.add_component_to_entity(entity, component2_data);

        application = application.add_system(writing_system);
        application = application.add_system(reading_writing_system);

        let (_, shutdown_receiver) = unbounded();
        application.run(RayonStaged, shutdown_receiver);

        let component_vec = application.world.borrow_component_vec().unwrap();
        let mut components = component_vec
            .iter()
            .filter_map(|c: &Option<TestComponent2>| c.as_ref());

        let result = TestComponent2(300);
        let first_component_data = components.next().unwrap();

        assert_eq!(&result, first_component_data);
    }

    #[test]
    fn rayon_staged_mutation_reversed() {
        let mut application: Application = Application::default();
        let entity = application.new_entity();
        let component1_data = TestComponent1(100);
        let component2_data = TestComponent2(200);
        application.add_component_to_entity(entity, component1_data);
        application.add_component_to_entity(entity, component2_data);

        application = application.add_system(reading_writing_system);
        application = application.add_system(writing_system);

        let (_, shutdown_receiver) = unbounded();
        application.run(RayonStaged, shutdown_receiver);

        let component_vec = application.world.borrow_component_vec().unwrap();
        let mut components = component_vec.iter().filter_map(|c| c.as_ref());

        let result = TestComponent2(250);
        let first_component_data = components.next().unwrap();

        assert_eq!(&result, first_component_data);
    }

    #[test]
    fn stage_generation_independent() {
        let mut parameters: SystemParametersVec = Vec::new();
        let t1 = std::any::type_name::<TestComponent1>();
        let t2 = std::any::type_name::<TestComponent2>();
        parameters.push((vec![], vec![t1.into()]));
        parameters.push((vec![], vec![t2.into()]));
        let stages = generate_stages(parameters);
        assert_eq!(stages.len(), 1);
    }

    #[test]
    fn stage_generation_dependent() {
        let mut parameters: SystemParametersVec = Vec::new();
        let t1 = std::any::type_name::<TestComponent1>();

        parameters.push((vec![], vec![t1.into()]));
        parameters.push((vec![t1.into()], vec![]));
        let stages = generate_stages(parameters);
        assert_eq!(stages.len(), 2);
    }

    #[test]
    fn stage_generation_dependent_added_after() {
        let mut parameters: SystemParametersVec = Vec::new();
        let t1 = std::any::type_name::<TestComponent1>();
        let t2 = std::any::type_name::<TestComponent2>();
        parameters.push((vec![], vec![t1.into()]));
        parameters.push((vec![], vec![t1.into(), t2.into()]));
        parameters.push((vec![], vec![t2.into()]));

        let stages = generate_stages(parameters);

        assert_eq!(stages.len(), 2);
    }

    #[test]
    fn dag_generation_independent() {
        let mut dag = DAG::new();
        let mut parameters: SystemParametersVec = Vec::new();
        let t1 = std::any::type_name::<TestComponent1>();
        let t2 = std::any::type_name::<TestComponent2>();
        parameters.push((vec![], vec![t1.into()]));
        parameters.push((vec![], vec![t2.into()]));

        dag.generate_dag(parameters);

        assert_eq!(dag.nodes, vec![0, 1]);
        assert_eq!(dag.edges, vec![])
    }

    #[test]
    fn dag_generation_dependent() {
        let mut dag = DAG::new();
        let mut parameters: SystemParametersVec = Vec::new();
        let t1 = std::any::type_name::<TestComponent1>();

        parameters.push((vec![], vec![t1.into()]));
        parameters.push((vec![t1.into()], vec![]));

        dag.generate_dag(parameters);

        assert_eq!(dag.nodes, vec![0, 1]);
        assert_eq!(dag.edges, vec![(0, 1)])
    }

    #[test] //This test shows that the DAG is not generated to get shortest path
    fn dag_generation_dependent_added_after() {
        let mut dag = DAG::new();
        let mut parameters: SystemParametersVec = Vec::new();
        let t1 = std::any::type_name::<TestComponent1>();
        let t2 = std::any::type_name::<TestComponent2>();
        parameters.push((vec![], vec![t1.into()]));
        parameters.push((vec![], vec![t1.into(), t2.into()]));
        parameters.push((vec![], vec![t2.into()]));

        dag.generate_dag(parameters);

        assert_eq!(dag.nodes, vec![0, 1, 2]);
        assert_eq!(dag.edges, vec![(0, 1), (1, 2)])
    }
}
