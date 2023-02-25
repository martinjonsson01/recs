use daggy::petgraph::algo;
use daggy::petgraph::visit::Bfs;
use daggy::{Dag, NodeIndex};

use ecs::System;

pub trait Schedule<'a> {
    fn generate(systems: &'a [Box<dyn System>]) -> Self;
}

type Sys<'a> = &'a Box<dyn System>;

#[derive(Debug, Default, Clone)]
pub struct DagSchedule<'a> {
    #[allow(clippy::borrowed_box)]
    dag: Dag<Sys<'a>, i32>,
}

impl<'a> PartialEq<Self> for DagSchedule<'a> {
    fn eq(&self, other: &Self) -> bool {
        let node_match = |a: &Sys<'a>, b: &Sys<'a>| a.id() == b.id();
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

        let mut previous_node = None;
        for system in systems {
            let node = dag.add_node(system);
            if let Some(previous_node) = previous_node {
                if let Some(same_access_node) =
                    find_node_with_component_access(&dag, previous_node, system.as_ref())
                {
                    dag.add_edge(node, same_access_node, 1)
                        .expect("Should not cycle");
                }
            }
            previous_node = Some(node);
        }

        Self { dag }
    }
}

fn find_node_with_component_access(
    dag: &Dag<Sys, i32>,
    begin_search_from: NodeIndex,
    system: &dyn System,
) -> Option<NodeIndex> {
    let type_ids = system.component_types();

    let mut bfs = Bfs::new(dag.graph(), begin_search_from);
    while let Some(node) = bfs.next(dag.graph()) {
        let system = dag
            .node_weight(node)
            .expect("Should be present since index was just gotten from BFS");

        let other_type_ids = system.component_types();
        if type_ids == other_type_ids {
            return Some(node);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use ecs::{Application, Read, Write};

    use super::*;

    macro_rules! assert_schedule_eq {
        ($a:expr, $b:expr, $message:expr) => {
            assert!(
                $a == $b,
                "{}\n\n{}  = {:?}\n{} = {:?}\n\n",
                format!($message),
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

        assert_schedule_eq!(DagSchedule::default(), schedule, "schedule should be empty");
    }

    #[derive(Debug, Default)]
    pub struct ComponentA(i32);
    #[derive(Debug, Default)]
    pub struct ComponentB(&'static str);

    fn read_a_system(_: Read<ComponentA>) {}
    fn read_b_system(_: Read<ComponentB>) {}
    fn write_a_system(_: Write<ComponentA>) {}
    fn write_b_system(_: Write<ComponentB>) {}

    #[test]
    fn schedule_does_not_connect_independent_systems() {
        let application = Application::default()
            .add_system(read_a_system)
            .add_system(read_b_system);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        expected_dag.add_node(&application.systems[0]);
        expected_dag.add_node(&application.systems[1]);

        let schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };
        assert_schedule_eq!(
            expected_schedule,
            schedule,
            "schedule should not connect independent systems"
        );
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
        expected_dag.add_edge(write_node, read_node, 1).unwrap();
        let read_node = expected_dag.add_node(&application.systems[2]);
        let write_node = expected_dag.add_node(&application.systems[3]);
        expected_dag.add_edge(write_node, read_node, 1).unwrap();

        let schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };
        assert_schedule_eq!(
            expected_schedule,
            schedule,
            "schedule should show component dependency as edge"
        );
    }
}
