use daggy::petgraph::dot::Dot;
use daggy::Dag;
use ecs::System;

pub trait Schedule {
    fn generate(systems: &[Box<dyn System>]) -> Self;
}

#[derive(Debug, Default, Clone)]
pub struct DagSchedule {
    dag: Dag<SystemNode, i32>,
}

impl PartialEq<Self> for DagSchedule {
    fn eq(&self, other: &Self) -> bool {
        let self_dot = format!("{:?}", Dot::new(self.dag.graph()));
        let other_dot = format!("{:?}", Dot::new(other.dag.graph()));
        self_dot == other_dot
    }
}

impl Schedule for DagSchedule {
    fn generate(systems: &[Box<dyn System>]) -> Self {
        let mut schedule = DagSchedule::default();

        for _system in systems {
            schedule.dag.add_node(SystemNode::Empty);
        }

        schedule
    }
}

#[derive(Debug, Default, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
enum SystemNode {
    #[default]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecs::Application;

    macro_rules! assert_schedule_eq {
        ($a:expr, $b:expr, $message:expr) => {
            assert!(
                $a == $b,
                "{}\n\n{}  = {:?}\n{} = {:?}\n\n",
                format!($message),
                stringify!($a),
                Dot::new($a.dag.graph()),
                stringify!($b),
                Dot::new($b.dag.graph()),
            )
        };
    }

    #[test]
    fn empty_systems_creates_empty_dag() {
        let systems = vec![];

        let schedule = DagSchedule::generate(&systems);

        assert_schedule_eq!(DagSchedule::default(), schedule, "did not match");
    }

    #[test]
    fn schedule_includes_system() {
        fn system() {
            println!("example system")
        }
        let application = Application::default().add_system(system);
        let mut expected_dag: Dag<SystemNode, i32> = Dag::new();
        expected_dag.add_node(SystemNode::Empty);

        let schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };
        assert_schedule_eq!(expected_schedule, schedule, "did not match");
    }
}
