use ecs::{Query, World};

fn main() {
    World::new()
        .add_system(basic_system)
        .add_system(system_with_parameter)
        .run();
}

fn basic_system() {
    println!("  Hello, world!")
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Position {
    x: f32,
    y: f32,
}

fn system_with_parameter(query: Query<Position>) {
    println!("  Hello from system with parameter {query:?}!")
}
