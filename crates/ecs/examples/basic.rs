use ecs::{Application, Query};

fn main() {
    let mut application = Application::default()
        .add_system(basic_system)
        .add_system(system_with_parameter)
        .add_system(system_with_two_parameters);

    let entity0 = application.new_entity();
    application.add_component_to_entity(entity0, Health(100));
    application.add_component_to_entity(entity0, Name("Somebody"));
    application.add_component_to_entity(entity0, Position { x: 1.0, y: 0.0 });

    let entity1 = application.new_entity();
    application.add_component_to_entity(entity1, Health(100));
    application.add_component_to_entity(entity1, Name("Somebody else"));
    application.add_component_to_entity(entity1, Position { x: 3., y: 4. });

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

fn system_with_parameter(query: Query<Position>) {
    println!("  Hello from system with parameter {:?}!", query.output)
}

fn system_with_two_parameters(query: Query<&Position>, empty: ()) {
    println!(
        "  Hello from system with two parameters {:?} and {empty:?}!",
        query.output
    )
}
