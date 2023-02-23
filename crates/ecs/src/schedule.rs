use crate::{Schedule, System, SystemParametersVec, World};
use crossbeam::channel::Receiver;
use rayon::prelude::*;
use std::collections::HashMap;
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
        self.nodes.push(node);
    }
    pub fn add_edge(&mut self, from: usize, to: usize) {
        self.edges.push((from, to));
    }

    /// Each stage consists of independent systems.
    /// A vector of tuples is returned with the first element
    /// being the index of the system in the systems vector.
    /// No user assigned ordering is taken into account. This is an avenue of development.
    /// Should be able to optimize by using a top down approach instead of a bottom up approach when inserting systems.
    /// Is quite slow for a large amount of systems without this optimization and maybe with.
    /// If a DAG is not needed and only stages are then the top down approach would be significantly better since
    /// fully searching for all conflicts would be unnecessary.
    /// In conclusion, this is a very naive implementation and needs review.
    pub fn generate_dag(
        &mut self,
        parameters: SystemParametersVec,
    ) -> Vec<EnumeratedSystemParametersVec> {
        let binding = parameters
            .iter()
            .enumerate()
            .map(|(i, x)| (i, x.clone()))
            .collect();
        let parameters_with_system_index: EnumeratedSystemParametersVec = binding;
        let mut stages: Vec<EnumeratedSystemParametersVec> = Vec::new();

        for system_current in parameters_with_system_index {
            let mut dependencies: HashMap<String, usize> = HashMap::new();
            let mut inserted = false;
            for stage in stages.iter_mut() {
                let mut conflict = false;
                for system in stage.iter() {
                    system_current.1 .0.iter().for_each(|component| {
                        if system.1 .1.iter().any(|x| x.deref() == component.deref()) {
                            conflict = true;
                            dependencies.insert(component.clone().into_string(), system.0);
                        }
                    });
                    system_current.1 .1.iter().for_each(|component| {
                        if system.1 .1.iter().any(|x| x.deref() == component.deref()) {
                            conflict = true;
                            dependencies.insert(component.clone().into_string(), system.0);
                        };
                    });
                }
                if !conflict {
                    self.add_node(system_current.0);
                    println!("Added node {}", system_current.0);
                    for dependency in &dependencies {
                        self.add_edge(*dependency.1, system_current.0);
                        println!("Added edge from {} to {}", dependency.1, system_current.0);
                    }
                    dependencies.clear();
                    stage.push(system_current.clone());
                    inserted = true;
                    break;
                }
            }
            if !inserted {
                self.add_node(system_current.0);
                println!("Added node {}", system_current.0);
                for dependency in &dependencies {
                    self.add_edge(*dependency.1, system_current.0);
                    println!("Added edge from {} to {}", dependency.1, system_current.0);
                }
                dependencies.clear();
                stages.push(vec![system_current.clone()]);
            }
        }
        println!("GENERATED STAGES: {stages:?}");
        println!("GENERATED EDGES: {:?}", self.edges);
        stages.to_vec()
    }
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
        let mut dag = DAG::new();
        let stages = dag.generate_dag(parameters.to_vec());
        loop {
            stages.iter().for_each(|stage| {
                stage
                    .par_iter()
                    .for_each(|k| systems[k.0].run_concurrent(world));
            });
        }
    }
}
