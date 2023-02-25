use daggy::petgraph::dot::Dot;
use daggy::Dag;
use ecs::System;

pub trait Schedule<'a> {
    fn generate(systems: &'a [Box<dyn System>]) -> Self;
}

#[derive(Debug, Default, Clone)]
pub struct DagSchedule<'a> {
    #[allow(clippy::borrowed_box)]
    dag: Dag<&'a Box<dyn System>, i32>,
}

impl<'a> PartialEq<Self> for DagSchedule<'a> {
    fn eq(&self, other: &Self) -> bool {
        let self_dot = format!("{:?}", Dot::new(self.dag.graph()));
        let other_dot = format!("{:?}", Dot::new(other.dag.graph()));
        self_dot == other_dot
    }
}

impl<'a> Schedule<'a> for DagSchedule<'a> {
    fn generate(systems: &'a [Box<dyn System>]) -> Self {
        let mut schedule = DagSchedule::default();

        for system in systems {
            schedule.dag.add_node(system);
        }

        schedule
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecs::{Application, Read, Write};

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
    fn schedule_includes_systems() {
        #[derive(Debug, Default)]
        pub struct ComponentA(i32);
        #[derive(Debug, Default)]
        pub struct ComponentB(&'static str);

        fn system_with_parameter(_: Read<ComponentA>) {
            println!("test system");
        }

        fn system_with_two_parameters(_: Read<ComponentA>, _: Write<ComponentB>) {
            println!("test system");
        }

        let application = Application::default()
            .add_system(system_with_parameter)
            .add_system(system_with_two_parameters);
        let mut expected_dag: Dag<&Box<dyn System>, i32> = Dag::new();
        expected_dag.add_node(&application.systems[0]);
        expected_dag.add_node(&application.systems[1]);

        let schedule = DagSchedule::generate(&application.systems);

        let expected_schedule = DagSchedule { dag: expected_dag };
        assert_schedule_eq!(expected_schedule, schedule, "did not match");
    }
}
