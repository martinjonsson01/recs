use ecs::World;

fn main() {
    World::new().add_system(basic_system).run();
}

fn basic_system() {
    println!("Hello, world!")
}
