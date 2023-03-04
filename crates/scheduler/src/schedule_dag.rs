use daggy::petgraph::visit::{IntoNeighbors, IntoNeighborsDirected, IntoNodeIdentifiers};
use daggy::petgraph::{algo, visit, Incoming};
use daggy::{Dag, NodeIndex};
use itertools::sorted;
use std::cmp::Ordering;

use ecs::scheduling::Schedule;
use ecs::System;

type Sys<'a> = &'a Box<dyn System>;

#[derive(Debug, Default, Clone)]
pub struct PrecedenceGraph<'a> {
    #[allow(clippy::borrowed_box)]
    pub dag: Dag<Sys<'a>, i32>,
    /// Warning: These indices are _not_ stable and will be invalidated if the dag is mutated.
    current_system_batch: Vec<NodeIndex>,
    /// Warning: These indices are _not_ stable and will be invalidated if the dag is mutated.
    already_executed: Vec<NodeIndex>,
}

impl<'a> PartialEq<Self> for PrecedenceGraph<'a> {
    fn eq(&self, other: &Self) -> bool {
        let node_match = |a: &Sys<'a>, b: &Sys<'a>| a == b;
        let edge_match = |a: &i32, b: &i32| a == b;
        algo::is_isomorphic_matching(
            &self.dag.graph(),
            &other.dag.graph(),
            node_match,
            edge_match,
        )
    }
}

impl<'a> Schedule<'a> for PrecedenceGraph<'a> {
    fn generate(systems: &'a [Box<dyn System>]) -> Self {
        let mut dag = Dag::new();

        for system in sorted(systems) {
            let node = dag.add_node(system);
            let dependent_nodes = find_nodes(&dag, node, system.as_ref(), Ordering::Greater);
            for dependent_on in dependent_nodes {
                dag.add_edge(node, dependent_on, 1)
                    .expect("Should not cycle");
            }
        }

        let (current_system_batch, _) = initial_systems(&dag);
        Self {
            dag,
            current_system_batch,
            already_executed: vec![],
        }
    }

    fn next_batch(&mut self) -> Vec<&'a dyn System> {
        let batch_nodes: Vec<_> = self
            .current_system_batch
            .iter()
            .flat_map(|&node| self.dag.neighbors(node))
            .filter(|&neighbor| {
                // Look at each system that depends on this one, and only if they have all
                // already executed include this one.
                visit::Reversed(&self.dag)
                    .neighbors(neighbor)
                    .all(|prerequisite| self.already_executed.contains(&prerequisite))
            })
            .collect();
        if batch_nodes.is_empty() {
            let (initial_nodes, initial_systems) = initial_systems(&self.dag);
            self.already_executed.extend(&initial_nodes);
            self.current_system_batch = initial_nodes;

            initial_systems
        } else {
            self.already_executed.extend(&batch_nodes);
            self.current_system_batch = batch_nodes.clone();

            nodes_to_systems(&self.dag, batch_nodes)
        }
    }
}
fn initial_systems<'a>(dag: &Dag<Sys<'a>, i32>) -> (Vec<NodeIndex>, Vec<&'a dyn System>) {
    let initial_nodes = dag
        .node_identifiers()
        .filter(|&node| dag.neighbors_directed(node, Incoming).next().is_none());
    let initial_systems = nodes_to_systems(dag, initial_nodes.clone());
    (initial_nodes.collect(), initial_systems)
}

fn nodes_to_systems<'a>(
    dag: &Dag<Sys<'a>, i32>,
    nodes: impl IntoIterator<Item = NodeIndex>,
) -> Vec<&'a dyn System> {
    nodes
        .into_iter()
        .filter_map(|node| dag.node_weight(node).map(|&system| system.as_ref()))
        .collect()
}

impl<'a> From<Dag<Sys<'a>, i32>> for PrecedenceGraph<'a> {
    fn from(dag: Dag<Sys<'a>, i32>) -> Self {
        let (current_system_batch, _) = initial_systems(&dag);
        Self {
            dag,
            current_system_batch,
            already_executed: vec![],
        }
    }
}

fn find_nodes(
    dag: &Dag<Sys, i32>,
    of_node: NodeIndex,
    system: &dyn System,
    order: Ordering,
) -> Vec<NodeIndex> {
    let mut dependent_on = vec![];

    for other_node_index in dag.graph().node_indices() {
        if other_node_index == of_node {
            continue;
        }

        let other_system = dag
            .node_weight(other_node_index)
            .expect("Should be present since index was just gotten from BFS");

        if system.cmp(other_system.as_ref()) == order {
            dependent_on.push(other_node_index)
        }
    }

    dependent_on
}

#[cfg(test)]
mod tests {
    use ecs::{Application, Read, Write};

    use super::*;

    macro_rules! assert_schedule_eq {
        ($a:expr, $b:expr, $message:expr) => {
            assert!(
                $a == $b,
                "{}\n\n{}  =\n{:?}\n{} =\n{:?}\n\n",
                format!($message),
                stringify!($a),
                daggy::petgraph::dot::Dot::new($a.dag.graph()),
                stringify!($b),
                daggy::petgraph::dot::Dot::new($b.dag.graph()),
            )
        };
        ($a:expr, $b:expr) => {
            assert!(
                $a == $b,
                "\n\n{}  =\n{:?}\n{} =\n{:?}\n\n",
                stringify!($a),
                daggy::petgraph::dot::Dot::new($a.dag.graph()),
                stringify!($b),
                daggy::petgraph::dot::Dot::new($b.dag.graph()),
            )
        };
    }

    #[test]
    fn empty_systems_creates_empty_dag() {
        let systems = vec![];

        let schedule = PrecedenceGraph::generate(&systems);

        assert_schedule_eq!(PrecedenceGraph::default(), schedule);
    }

    #[derive(Debug, Default)]
    pub struct A(i32);
    #[derive(Debug, Default)]
    pub struct B(&'static str);
    #[derive(Debug, Default)]
    pub struct C(f32);

    fn read_a_system(_: Read<A>) {}
    fn read_b_system(_: Read<B>) {}
    fn read_c_system(_: Read<C>) {}
    fn write_a_system(_: Write<A>) {}
    fn write_b_system(_: Write<B>) {}

    fn write_ab_system(_: Write<A>, _: Write<B>) {}
    fn read_ab_system(_: Read<A>, _: Read<B>) {}
    fn read_b_write_a_system(_: Read<B>, _: Write<A>) {}
    fn read_a_write_b_system(_: Read<A>, _: Write<B>) {}
    fn read_a_write_c_system(_: Read<A>, _: Write<C>) {}

    #[test]
    fn schedule_does_not_connect_independent_systems() {
        let application = Application::default()
            .add_system(read_a_system)
            .add_system(read_b_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        expected_dag.add_node(&application.systems[0]);
        expected_dag.add_node(&application.systems[1]);

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();
        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_represents_component_dependencies_as_edges() {
        let application = Application::default()
            .add_system(read_a_system)
            .add_system(write_a_system)
            .add_system(read_b_system)
            .add_system(write_b_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        let read_node = expected_dag.add_node(&application.systems[0]);
        let write_node = expected_dag.add_node(&application.systems[1]);
        expected_dag.add_edge(read_node, write_node, 1).unwrap();
        let read_node = expected_dag.add_node(&application.systems[2]);
        let write_node = expected_dag.add_node(&application.systems[3]);
        expected_dag.add_edge(read_node, write_node, 1).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();
        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_allows_multiple_reads_to_run_simultaneously() {
        let application = Application::default()
            .add_system(read_a_system)
            .add_system(read_a_system)
            .add_system(write_a_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        let read_node0 = expected_dag.add_node(&application.systems[0]);
        let read_node1 = expected_dag.add_node(&application.systems[1]);
        let write_node = expected_dag.add_node(&application.systems[2]);
        expected_dag.add_edge(read_node0, write_node, 1).unwrap();
        expected_dag.add_edge(read_node1, write_node, 1).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();
        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_ignores_non_overlapping_components() {
        let application = Application::default()
            .add_system(read_a_system)
            .add_system(read_a_write_c_system)
            .add_system(read_b_write_a_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        let read_node0 = expected_dag.add_node(&application.systems[0]);
        let read_node1 = expected_dag.add_node(&application.systems[1]);
        let write_node = expected_dag.add_node(&application.systems[2]);
        expected_dag.add_edge(read_node0, write_node, 1).unwrap();
        expected_dag.add_edge(read_node1, write_node, 1).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_places_multiple_writes_in_sequence() {
        let application = Application::default()
            .add_system(read_a_system)
            .add_system(write_a_system)
            .add_system(write_a_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        let read_node = expected_dag.add_node(&application.systems[0]);
        let write_node0 = expected_dag.add_node(&application.systems[1]);
        let write_node1 = expected_dag.add_node(&application.systems[2]);
        expected_dag.add_edge(write_node1, write_node0, 1).unwrap();
        expected_dag.add_edge(read_node, write_node1, 1).unwrap();
        expected_dag.add_edge(read_node, write_node0, 1).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_allows_disjoint_concurrent_component_writes() {
        let application = Application::default()
            .add_system(write_a_system)
            .add_system(read_a_system)
            .add_system(write_b_system)
            .add_system(read_b_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        let write_node0 = expected_dag.add_node(&application.systems[0]);
        let read_node0 = expected_dag.add_node(&application.systems[1]);
        let write_node1 = expected_dag.add_node(&application.systems[2]);
        let read_node1 = expected_dag.add_node(&application.systems[3]);
        expected_dag.add_edge(read_node0, write_node0, 1).unwrap();
        expected_dag.add_edge(read_node1, write_node1, 1).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(
            expected_schedule,
            actual_schedule,
            "systems should be able to write to different component types simultaneously"
        );
    }

    #[test]
    fn preserves_precedence_in_multi_stage_schedule() {
        let application = Application::default()
            .add_system(write_ab_system)
            .add_system(write_b_system)
            .add_system(write_a_system)
            .add_system(read_b_system)
            .add_system(read_a_system)
            .add_system(read_a_write_c_system)
            .add_system(read_c_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        let write_ab = expected_dag.add_node(&application.systems[0]);
        let write_b = expected_dag.add_node(&application.systems[1]);
        let write_a = expected_dag.add_node(&application.systems[2]);
        let read_b = expected_dag.add_node(&application.systems[3]);
        let read_a = expected_dag.add_node(&application.systems[4]);
        let read_a_write_c = expected_dag.add_node(&application.systems[5]);
        let read_c = expected_dag.add_node(&application.systems[6]);
        // Stage 1 (all except read_c_system depend on write_ab_system)
        expected_dag.add_edge(write_b, write_ab, 1).unwrap();
        expected_dag.add_edge(write_a, write_ab, 1).unwrap();
        expected_dag.add_edge(read_b, write_ab, 1).unwrap();
        expected_dag.add_edge(read_a, write_ab, 1).unwrap();
        expected_dag.add_edge(read_a_write_c, write_ab, 1).unwrap();
        // Stage 2
        expected_dag.add_edge(read_b, write_b, 1).unwrap();
        expected_dag.add_edge(read_a, write_a, 1).unwrap();
        expected_dag.add_edge(read_a_write_c, write_a, 1).unwrap();
        // Stage 3
        expected_dag.add_edge(read_c, read_a_write_c, 1).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn multiple_writes_are_placed_in_sequence_for_multicomponent_systems() {
        let application = Application::default()
            .add_system(read_ab_system)
            .add_system(write_ab_system)
            .add_system(read_b_write_a_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        let read_ab = expected_dag.add_node(&application.systems[0]);
        let write_ab = expected_dag.add_node(&application.systems[1]);
        let read_b_write_a = expected_dag.add_node(&application.systems[2]);
        expected_dag.add_edge(read_ab, write_ab, 1).unwrap();
        expected_dag.add_edge(read_b_write_a, write_ab, 1).unwrap();
        expected_dag.add_edge(read_ab, read_b_write_a, 1).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn prevents_deadlock_by_scheduling_deadlocking_accesses_sequentially() {
        let application = Application::default()
            .add_system(read_a_write_b_system)
            .add_system(read_b_write_a_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        let read_a_write_b = expected_dag.add_node(&application.systems[0]);
        let read_b_write_a = expected_dag.add_node(&application.systems[1]);
        expected_dag
            .add_edge(read_a_write_b, read_b_write_a, 1)
            .unwrap();

        let actual_schedule = PrecedenceGraph::generate(&application.systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn dag_execution_traversal_begins_with_systems_without_preceding_systems() {
        let application = Application::default()
            .add_system(write_ab_system)
            .add_system(write_b_system)
            .add_system(write_a_system)
            .add_system(read_b_system)
            .add_system(read_a_system)
            .add_system(read_a_write_c_system)
            .add_system(read_c_system);
        let mut schedule = PrecedenceGraph::generate(&application.systems);

        let _write_ab_system = application.systems[0].as_ref();
        let _write_b_system = application.systems[1].as_ref();
        let _write_a_system = application.systems[2].as_ref();
        let read_b_system = application.systems[3].as_ref();
        let read_a_system = application.systems[4].as_ref();
        let _read_a_write_c_system = application.systems[5].as_ref();
        let read_c_system = application.systems[6].as_ref();
        let expected_first_batch = vec![read_c_system, read_a_system, read_b_system];

        let first_batch = schedule.next_batch();

        assert_eq!(expected_first_batch, first_batch);
    }

    #[test]
    fn dag_execution_traversal_gives_entire_layers_of_executable_systems() {
        let application = Application::default()
            .add_system(write_ab_system)
            .add_system(write_b_system)
            .add_system(write_a_system)
            .add_system(read_b_system)
            .add_system(read_a_system)
            .add_system(read_a_write_c_system)
            .add_system(read_c_system);
        let mut schedule = PrecedenceGraph::generate(&application.systems);

        let write_ab_system = application.systems[0].as_ref();
        let write_b_system = application.systems[1].as_ref();
        let write_a_system = application.systems[2].as_ref();
        let _read_b_system = application.systems[3].as_ref();
        let _read_a_system = application.systems[4].as_ref();
        let read_a_write_c_system = application.systems[5].as_ref();
        let _read_c_system = application.systems[6].as_ref();
        let expected_second_batch = vec![write_b_system, read_a_write_c_system];
        let expected_third_batch = vec![write_a_system];
        let expected_final_batch = vec![write_ab_system];

        drop(schedule.next_batch());
        let second_batch = schedule.next_batch();
        let third_batch = schedule.next_batch();
        let final_batch = schedule.next_batch();

        assert_eq!(expected_second_batch, second_batch);
        assert_eq!(expected_third_batch, third_batch);
        assert_eq!(expected_final_batch, final_batch);
    }

    #[test]
    fn dag_execution_repeats_once_fully_executed() {
        let application = Application::default()
            .add_system(read_a_system)
            .add_system(write_a_system);
        let mut schedule = PrecedenceGraph::generate(&application.systems);

        let read_a = application.systems[0].as_ref();
        let write_a = application.systems[1].as_ref();

        let first_batch = schedule.next_batch();
        let second_batch = schedule.next_batch();
        let third_batch = schedule.next_batch();

        assert_eq!(vec![read_a], first_batch);
        assert_eq!(vec![write_a], second_batch);
        assert_eq!(vec![read_a], third_batch);
    }
}
