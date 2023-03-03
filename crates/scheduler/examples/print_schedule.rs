use daggy::petgraph::dot::Dot;
use ecs::scheduling::Schedule;
use ecs::{Application, Read, Write};
use scheduler::schedule_dag::DagSchedule;
use std::thread;
use std::time::Duration;

fn main() {
    let application: Application = Application::default()
        .add_system(basic_system)
        .add_system(system_with_parameter)
        .add_system(system_with_two_parameters)
        .add_system(system_with_two_mutable_parameters)
        .add_system(system_with_read_and_write);

    let actual_schedule = DagSchedule::generate(&application.systems);
    println!("{:?}", Dot::new(actual_schedule.dag.graph()));
}

fn basic_system() {
    println!("  Hello, world!");
    thread::sleep(Duration::from_micros(100));
}

#[derive(Debug, Default)]
pub struct Health(pub i32);
#[derive(Debug, Default)]
pub struct Name(pub &'static str);

#[derive(Debug, Copy, Clone, PartialEq)]
struct Position {
    x: f32,
    y: f32,
}

fn system_with_parameter(_: Read<Position>) {}

fn system_with_two_parameters(_: Read<Name>, _: Read<Health>) {}

fn system_with_two_mutable_parameters(_: Write<Name>, _: Write<Health>) {}

fn system_with_read_and_write(_: Read<Name>, _: Write<Health>) {}
