use crossbeam::channel::unbounded;
use ecs::schedule::RayonStaged;
use ecs::{thread_println, Application, Read, Write};
use std::thread;

fn main() {
    let mut application: Application = Application::default()
        .add_system(basic_system)
        .add_system(system_with_parameter)
        .add_system(system_with_two_parameters)
        .add_system(system_with_two_mutable_parameters)
        .add_system(system_with_read_and_write)
        .add_system(system_with_two_parameters);
    for _k in 0..10 {
        application = application.add_system(system_with_read_and_write);
    }

    for k in 0..100 {
        let entity = application.new_entity();
        application.add_component_to_entity(entity, Health(k));
        application.add_component_to_entity(entity, Name("Somebody"));
        application.add_component_to_entity(
            entity,
            Position {
                x: 100.0 - (k as f32),
                y: k as f32,
            },
        );
    }

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    application.run(RayonStaged, shutdown_receiver)
}

fn basic_system() {
    println!("  Hello, world!");
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

fn system_with_parameter(query: Read<Position>) {
    thread_println!("  Hello from system with parameter {:?}!", query.output);
}

fn system_with_two_parameters(pos: Read<Name>, health: Read<Health>) {
    thread_println!(
        "  Hello from system with two parameters {:?} and {:?}!",
        pos.output,
        health.output
    );
}

fn system_with_two_mutable_parameters(name: Write<Name>, health: Write<Health>) {
    thread_println!(
        "  Hello from system with two mutable parameters {:?} and {:?} .. ",
        name.output,
        health.output
    );
    *name.output = Name("dead!");
    *health.output = Health(0);
    thread_println!("mutated to {:?} and {:?}!", name.output, health.output);
}

fn system_with_read_and_write(name: Read<Name>, health: Write<Health>) {
    thread_println!(
        "  Hello from system with one mutable and one immutable parameter {:?} and {:?} .. ",
        name.output,
        health.output
    );
    *health.output = Health(99);
    thread_println!("mutated to {:?} and {:?}!", name.output, health.output);
}
