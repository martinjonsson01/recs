use ecs::{Application, Query, Without};

fn main() {
    let mut application = Application::default()
        .add_system(basic_system)
        .add_system(system_with_parameter)
        .add_system(system_with_two_parameters)
        .add_system(system_with_filter);

    let entity0 = application.new_entity();
    application.add_component_to_entity(entity0, Health(100));
    application.add_component_to_entity(entity0, Name("Somebody"));
    application.add_component_to_entity(entity0, Position { x: 1.0, y: 0.0 });

    let entity1 = application.new_entity();
    application.add_component_to_entity(entity1, Health(100));
    application.add_component_to_entity(entity1, Name("Somebody else"));
    application.add_component_to_entity(entity1, Position { x: 3., y: 4. });

    let entity2 = application.new_entity();
    application.add_component_to_entity(entity2, Health(100));
    application.add_component_to_entity(entity2, Position { x: 5., y: 6. });

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

fn system_with_parameter(position: Query<Position>) {
    println!("  Hello from system with parameter {:?}!", position);
}

fn system_with_two_parameters(position: Query<Position>, health: Query<Health>) {
    println!(
        "  Hello from system with two parameters {:?} and {:?}!",
        position, health
    )
}

fn system_with_filter(position: Query<Position>, _: Without<Name>) {
    println!(
        "  Hello from system with parameter {:?} and with filter Without<Name>!",
        position
    );
}
