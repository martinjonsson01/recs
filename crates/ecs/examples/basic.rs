use ecs::{Application, Read, Write};

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
    application.add_component_to_entity(entity0, Health(100));
    application.add_component_to_entity(entity0, Name("Somebody"));
    application.add_component_to_entity(entity0, Position { x: 1.0, y: 0.0 });
    application.add_component_to_entity(entity1, Health(43));
    application.add_component_to_entity(entity1, Name("Somebody else"));
    application.add_component_to_entity(entity1, Position { x: 2.0, y: 3.0 });

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
    println!("  Hello from system with parameter {:?}!", query.output)
}

fn system_with_two_parameters(pos: Read<Name>, health: Read<Health>) {
    println!(
        "  Hello from system with two parameters {:?} and {:?}!",
        pos.output, health.output
    )
}

fn system_with_two_mutable_parameters(name: Write<Name>, health: Write<Health>) {
    print!(
        "  Hello from system with two mutable parameters {:?} and {:?} .. ",
        name.output, health.output
    );
    *name.output = Name("dead!");
    *health.output = Health(0);
    println!("mutated to {:?} and {:?}!", name.output, health.output);
}

fn system_with_read_and_write(name: Read<Name>, health: Write<Health>) {
    print!(
        "  Hello from system with one mutable and one immutable parameter {:?} and {:?} .. ",
        name.output, health.output
    );
    *health.output = Health(99);
    println!("mutated to {:?} and {:?}!", name.output, health.output);
}
