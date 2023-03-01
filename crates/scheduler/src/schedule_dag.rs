use daggy::petgraph::algo;
use daggy::{Dag, NodeIndex};
use itertools::sorted;
use std::cmp::Ordering;

use ecs::{Schedule, System};

type Sys<'a> = &'a Box<dyn System>;

#[derive(Debug, Default, Clone)]
pub struct DagSchedule<'a> {
    #[allow(clippy::borrowed_box)]
    pub dag: Dag<Sys<'a>, i32>,
}

impl<'a> PartialEq<Self> for DagSchedule<'a> {
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

impl<'a> Schedule<'a> for DagSchedule<'a> {
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

        Self { dag }
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

        let schedule = DagSchedule::generate(&systems);

        assert_schedule_eq!(DagSchedule::default(), schedule);
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
    fn read_a_write_c_system(_: Read<A>, _: Write<C>) {}

    #[test]
    fn schedule_does_not_connect_independent_systems() {
        let application = Application::default()
            .add_system(read_a_system)
            .add_system(read_b_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        expected_dag.add_node(&application.systems[0]);
        expected_dag.add_node(&application.systems[1]);

        let actual_schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };
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

        let actual_schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };
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

        let actual_schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };
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

        let actual_schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };

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

        let actual_schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };

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

        let actual_schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };

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

        let actual_schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };

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

        let actual_schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };

        assert_schedule_eq!(expected_schedule, actual_schedule);
    }
}
