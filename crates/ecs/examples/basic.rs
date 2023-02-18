use ecs::{Application, Read, Write};
use std::{thread, time::Duration};

fn main() {
    let mut application = Application::default()
        .add_system(basic_system)
        .add_system(system_with_parameter)
        .add_system(system_with_two_parameters)
        .add_system(system_with_two_mutable_parameters)
        .add_system(system_with_read_and_write)
        .add_system(system_with_two_parameters);

    let entity0 = application.new_entity();
    let entity1 = application.new_entity();
    let entity2 = application.new_entity();
    let entity3 = application.new_entity();
    let entity4 = application.new_entity();
    let entity5 = application.new_entity();
    let entity6 = application.new_entity();
    let entity7 = application.new_entity();
    let entity8 = application.new_entity();
    let entity9 = application.new_entity();

    application.add_component_to_entity(entity0, Health(100));
    application.add_component_to_entity(entity0, Name("Somebody"));
    application.add_component_to_entity(entity0, Position { x: 1.0, y: 0.0 });
    application.add_component_to_entity(entity1, Health(43));
    application.add_component_to_entity(entity1, Name("Somebody else"));
    application.add_component_to_entity(entity1, Position { x: 2.0, y: 3.0 });
    application.add_component_to_entity(entity2, Health(100));
    application.add_component_to_entity(entity2, Name("Somebody"));
    application.add_component_to_entity(entity2, Position { x: 1.0, y: 0.0 });
    application.add_component_to_entity(entity3, Health(43));
    application.add_component_to_entity(entity3, Name("Somebody else"));
    application.add_component_to_entity(entity3, Position { x: 2.0, y: 3.0 });
    application.add_component_to_entity(entity4, Health(100));
    application.add_component_to_entity(entity4, Name("Somebody"));
    application.add_component_to_entity(entity4, Position { x: 1.0, y: 0.0 });
    application.add_component_to_entity(entity5, Health(43));
    application.add_component_to_entity(entity5, Name("Somebody else"));
    application.add_component_to_entity(entity5, Position { x: 2.0, y: 3.0 });
    application.add_component_to_entity(entity6, Health(100));
    application.add_component_to_entity(entity6, Name("Somebody"));
    application.add_component_to_entity(entity6, Position { x: 1.0, y: 0.0 });
    application.add_component_to_entity(entity7, Health(43));
    application.add_component_to_entity(entity7, Name("Somebody else"));
    application.add_component_to_entity(entity7, Position { x: 2.0, y: 3.0 });
    application.add_component_to_entity(entity8, Health(100));
    application.add_component_to_entity(entity8, Name("Somebody"));
    application.add_component_to_entity(entity8, Position { x: 1.0, y: 0.0 });
    application.add_component_to_entity(entity9, Health(43));
    application.add_component_to_entity(entity9, Name("Somebody else"));
    application.add_component_to_entity(entity9, Position { x: 2.0, y: 3.0 });

    application.run()
}

fn basic_system() {
    println!("  Hello, world!")
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
    println!("  Hello from system with parameter {:?}!", query.output);
    thread::sleep(Duration::from_millis(1000));
    println!("  Goodbye from system with parameter {:?}!", query.output);


}

fn system_with_two_parameters(pos: Read<Name>, health: Read<Health>) {
    println!(
        "  Hello from system with two parameters {:?} and {:?}!",
        pos.output, health.output
    );
    thread::sleep(Duration::from_millis(1000));
    println!(
        "  Goodbye from system with two parameters {:?} and {:?}!",
        pos.output, health.output
    );

}

fn system_with_two_mutable_parameters(name: Write<Name>, health: Write<Health>) {
    println!(
        "  Hello from system with two mutable parameters {:?} and {:?} .. ",
        name.output, health.output
    );
    *name.output = Name("dead!");
    *health.output = Health(0);
    thread::sleep(Duration::from_millis(1000));

    println!("mutated to {:?} and {:?}!", name.output, health.output);
}

fn system_with_read_and_write(name: Read<Name>, health: Write<Health>) {
    println!(
        "  Hello from system with one mutable and one immutable parameter {:?} and {:?} .. ",
        name.output, health.output
    );
    *health.output = Health(99);
    thread::sleep(Duration::from_millis(1000));
    println!("mutated to {:?} and {:?}!", name.output, health.output);
}
