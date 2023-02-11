use ecs::{Query, World};

fn main() {
    let mut world = World::new()
        .add_system(basic_system)
        .add_system(system_with_parameter)
        .add_system(system_with_two_parameters);

    let entity0 = world.new_entity();
    world.add_component_to_entity(entity0, Health(100));
    world.add_component_to_entity(entity0, Name("Somebody"));

    world.run()
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

fn system_with_parameter(query: Query<&Position>) {
    println!("  Hello from system with parameter {query:?}!")
}

fn system_with_two_parameters(query: Query<&Position>, empty: ()) {
    println!("  Hello from system with two parameters {query:?} and {empty:?}!")
}
