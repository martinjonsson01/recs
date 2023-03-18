//! Schedules that produce orderings of systems that are correct.
//!
//! Correct, meaning they (try to) guarantee:
//! * freedom from system starvation, meaning all systems get to execute in the schedule ordering;
//! * freedom from race conditions, meaning the ordering will not place reads and writes to
//!   the same component at the same time;
//! * freedom from deadlock, meaning systems that are ordered such that they will always
//!   be able to progress.

use crate::precedence::{Orderable, Precedence};
use daggy::{Dag, NodeIndex};
use ecs::{Schedule, System, SystemExecutionGuard};
use std::fmt::{Debug, Formatter};

type Sys<'system> = &'system dyn System;
type SysDag<'system> = Dag<Sys<'system>, ()>;

/// Orders [`System`]s based on their precedence.
#[derive(Default, Clone)]
pub struct PrecedenceGraph<'systems> {
    dag: Dag<Sys<'systems>, ()>,
}

impl<'systems> Debug for PrecedenceGraph<'systems> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Instead of printing the struct, format the DAG in dot-format and print that.
        // (so it can be viewed through tools like http://viz-js.com/)
        writeln!(f, "{:?}", daggy::petgraph::dot::Dot::new(self.dag.graph()))
    }
}

// PrecedenceGraph-equality should be based on whether the underlying DAGs are
// isomorphic or not - i.e. whether they're equivalently constructed but not necessarily
// exactly the same.
impl<'systems> PartialEq<Self> for PrecedenceGraph<'systems> {
    fn eq(&self, other: &Self) -> bool {
        let node_match = |a: &Sys<'systems>, b: &Sys<'systems>| a == b;
        let edge_match = |_: &(), _: &()| true;
        daggy::petgraph::algo::is_isomorphic_matching(
            &self.dag.graph(),
            &other.dag.graph(),
            node_match,
            edge_match,
        )
    }
}

impl<'systems> Schedule<'systems> for PrecedenceGraph<'systems> {
    fn generate(systems: &'systems [Box<dyn System>]) -> Self {
        let mut dag = Dag::new();

        for system in systems.iter().map(|system| system.as_ref()) {
            let node = dag.add_node(system);

            // Find nodes which current system must run _after_ (i.e. current has dependency on).
            let should_run_after = find_nodes(&dag, node, system, Precedence::After);
            for dependent_on in should_run_after {
                dag.add_edge(node, dependent_on, ())
                    .expect("DAG generation algorithm should not ever create cycles");
            }

            // Find nodes which current system must run _before_ (i.e. have a dependency on current).
            let should_run_before = find_nodes(&dag, node, system, Precedence::Before);
            for dependency_of in should_run_before {
                dag.add_edge(dependency_of, node, ())
                    .expect("DAG generation algorithm should not ever create cycles");
            }
        }

        Self { dag }
    }

    fn currently_executable_systems(&mut self) -> Vec<SystemExecutionGuard<'systems>> {
        todo!()
    }

    fn system_completed_execution(&mut self, _system: &dyn System) {
        todo!()
    }
}

fn find_nodes(
    dag: &SysDag,
    of_node: NodeIndex,
    system: Sys,
    precedence: Precedence,
) -> Vec<NodeIndex> {
    let mut found_nodes = vec![];

    for other_node_index in dag.graph().node_indices() {
        if other_node_index == of_node {
            continue;
        }

        let &other_system = dag
            .node_weight(other_node_index)
            .expect("Node should exist since its index was just fetched from the graph");

        if system.precedence_to(other_system) == precedence {
            found_nodes.push(other_node_index);
        }
    }

    found_nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use daggy::petgraph::dot::Dot;
    use test_log::test;
    use test_strategy::proptest;
    use test_utils::{
        arb_systems, into_system, read_a, read_a_write_b, read_a_write_c, read_ab, read_b,
        read_b_write_a, read_c, write_a, write_ab, write_b,
    };

    // Easily convert from a DAG to a PrecedenceGraph, just for simpler tests.
    impl<'a> From<SysDag<'a>> for PrecedenceGraph<'a> {
        fn from(dag: SysDag<'a>) -> Self {
            Self { dag }
        }
    }

    #[proptest]
    fn precedence_graphs_are_equal_if_isomorphic(
        #[strategy(arb_systems(3, 3))] systems: Vec<Box<dyn System>>,
    ) {
        let mut a = PrecedenceGraph::default();
        let a_node0 = a.dag.add_node(systems[0].as_ref());
        let a_node1 = a.dag.add_node(systems[1].as_ref());
        let a_node2 = a.dag.add_node(systems[2].as_ref());
        a.dag
            .add_edges([(a_node0, a_node1, ()), (a_node0, a_node2, ())])
            .unwrap();

        let mut b = PrecedenceGraph::default();
        let b_node0 = b.dag.add_node(systems[0].as_ref());
        let b_node1 = b.dag.add_node(systems[1].as_ref());
        let b_node2 = b.dag.add_node(systems[2].as_ref());
        // Same edges as a but added in reverse order.
        b.dag
            .add_edges([(b_node0, b_node2, ()), (b_node0, b_node1, ())])
            .unwrap();

        // When comparing the raw contents (i.e. exact same order of edges and same indices of nodes)
        // they should not be equal, since edges were added in different orders.
        let a_dot = format!("{:?}", Dot::new(a.dag.graph()));
        let b_dot = format!("{:?}", Dot::new(b.dag.graph()));
        assert_ne!(a_dot, b_dot);

        // But they are isomorphic.
        assert_eq!(a, b);
    }

    macro_rules! assert_schedule_eq {
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
    fn schedule_does_not_connect_independent_systems() {
        let systems = [into_system(read_a), into_system(read_b)];
        let mut expected_dag: SysDag = Dag::new();
        expected_dag.add_node(systems[0].as_ref());
        expected_dag.add_node(systems[1].as_ref());

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();
        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_represents_component_dependencies_as_edges() {
        let systems = [
            into_system(read_a),
            into_system(write_a),
            into_system(read_b),
            into_system(write_b),
        ];
        let mut expected_dag: SysDag = Dag::new();
        let read_a = expected_dag.add_node(systems[0].as_ref());
        let write_a = expected_dag.add_node(systems[1].as_ref());
        expected_dag.add_edge(read_a, write_a, ()).unwrap();
        let read_b = expected_dag.add_node(systems[2].as_ref());
        let write_b = expected_dag.add_node(systems[3].as_ref());
        expected_dag.add_edge(read_b, write_b, ()).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();
        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_allows_multiple_reads_to_run_simultaneously() {
        let systems = [
            into_system(read_a),
            into_system(read_a),
            into_system(write_a),
        ];
        let mut expected_dag: SysDag = Dag::new();
        let read_a0 = expected_dag.add_node(systems[0].as_ref());
        let read_a1 = expected_dag.add_node(systems[1].as_ref());
        let write_a = expected_dag.add_node(systems[2].as_ref());
        expected_dag.add_edge(read_a0, write_a, ()).unwrap();
        expected_dag.add_edge(read_a1, write_a, ()).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();
        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_ignores_non_overlapping_components() {
        let systems = [
            into_system(read_a),
            into_system(read_a_write_c),
            into_system(read_b_write_a),
        ];
        let mut expected_dag: SysDag = Dag::new();
        let read_node0 = expected_dag.add_node(systems[0].as_ref());
        let read_node1 = expected_dag.add_node(systems[1].as_ref());
        let write_node = expected_dag.add_node(systems[2].as_ref());
        expected_dag.add_edge(read_node0, write_node, ()).unwrap();
        expected_dag.add_edge(read_node1, write_node, ()).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_places_multiple_writes_in_sequence() {
        let systems = [
            into_system(read_a),
            into_system(write_a),
            into_system(write_a),
        ];
        let mut expected_dag: SysDag = Dag::new();
        let read_node = expected_dag.add_node(systems[0].as_ref());
        let write_node0 = expected_dag.add_node(systems[1].as_ref());
        let write_node1 = expected_dag.add_node(systems[2].as_ref());
        expected_dag.add_edge(write_node1, write_node0, ()).unwrap();
        expected_dag.add_edge(read_node, write_node1, ()).unwrap();
        expected_dag.add_edge(read_node, write_node0, ()).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn schedule_allows_concurrent_component_writes_to_separate_components() {
        let systems = [
            into_system(write_a),
            into_system(read_a),
            into_system(write_b),
            into_system(read_b),
        ];
        let mut expected_dag: SysDag = Dag::new();
        let write_node0 = expected_dag.add_node(systems[0].as_ref());
        let read_node0 = expected_dag.add_node(systems[1].as_ref());
        let write_node1 = expected_dag.add_node(systems[2].as_ref());
        let read_node1 = expected_dag.add_node(systems[3].as_ref());
        expected_dag.add_edge(read_node0, write_node0, ()).unwrap();
        expected_dag.add_edge(read_node1, write_node1, ()).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn preserves_precedence_in_multi_layer_schedule() {
        let systems = [
            into_system(write_ab),
            into_system(write_b),
            into_system(write_a),
            into_system(read_b),
            into_system(read_a),
            into_system(read_a_write_c),
            into_system(read_c),
        ];
        let mut expected_dag: SysDag = Dag::new();
        let write_ab = expected_dag.add_node(systems[0].as_ref());
        let write_b = expected_dag.add_node(systems[1].as_ref());
        let write_a = expected_dag.add_node(systems[2].as_ref());
        let read_b = expected_dag.add_node(systems[3].as_ref());
        let read_a = expected_dag.add_node(systems[4].as_ref());
        let read_a_write_c = expected_dag.add_node(systems[5].as_ref());
        let read_c = expected_dag.add_node(systems[6].as_ref());
        // "Layer" 1 (all except read_c_system depend on write_ab_system)
        expected_dag.add_edge(write_b, write_ab, ()).unwrap();
        expected_dag.add_edge(write_a, write_ab, ()).unwrap();
        expected_dag.add_edge(read_b, write_ab, ()).unwrap();
        expected_dag.add_edge(read_a, write_ab, ()).unwrap();
        expected_dag.add_edge(read_a_write_c, write_ab, ()).unwrap();
        // "Layer" 2
        expected_dag.add_edge(read_b, write_b, ()).unwrap();
        expected_dag.add_edge(read_a, write_a, ()).unwrap();
        expected_dag.add_edge(read_a_write_c, write_a, ()).unwrap();
        // "Layer" 3
        expected_dag.add_edge(read_c, read_a_write_c, ()).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn multiple_writes_are_placed_in_sequence_for_multicomponent_systems() {
        let systems = [
            into_system(read_ab),
            into_system(write_ab),
            into_system(read_b_write_a),
        ];
        let mut expected_dag: SysDag = Dag::new();
        let read_ab = expected_dag.add_node(systems[0].as_ref());
        let write_ab = expected_dag.add_node(systems[1].as_ref());
        let read_b_write_a = expected_dag.add_node(systems[2].as_ref());
        expected_dag.add_edge(read_ab, write_ab, ()).unwrap();
        expected_dag.add_edge(read_b_write_a, write_ab, ()).unwrap();
        expected_dag.add_edge(read_ab, read_b_write_a, ()).unwrap();

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }

    #[test]
    fn prevents_deadlock_by_scheduling_deadlocking_accesses_sequentially() {
        let systems = [into_system(read_a_write_b), into_system(read_b_write_a)];
        let mut expected_dag: SysDag = Dag::new();
        let read_a_write_b = expected_dag.add_node(systems[0].as_ref());
        let read_b_write_a = expected_dag.add_node(systems[1].as_ref());
        expected_dag
            .add_edge(read_b_write_a, read_a_write_b, ())
            .unwrap();

        let actual_schedule = PrecedenceGraph::generate(&systems);

        let expected_schedule: PrecedenceGraph = expected_dag.into();

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }
}
