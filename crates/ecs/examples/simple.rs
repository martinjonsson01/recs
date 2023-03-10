use crossbeam::channel::unbounded;
use ecs::{Application, Read, Sequential, Unordered, Write};

// a simple example of how to use the crate `ecs`
fn main() {
    let mut app = Application::default()
        .add_system(basic_system)
        .add_system(read_a_system)
        .add_system(write_a_system)
        .add_system(read_write_many);

    for i in 0..10 {
        let entity = app.create_entity();
        app.add_component(entity, A(i));
        app.add_component(entity, B(i));
        app.add_component(entity, C);
        app.add_component(entity, D);
        app.add_component(entity, E);
    }

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<Sequential, Unordered>(shutdown_receiver);
}

#[derive(Debug)]
struct A(i32);

#[derive(Debug)]
struct B(i32);

#[derive(Debug)]
struct C;

#[derive(Debug)]
struct D;

#[derive(Debug)]
struct E;

fn basic_system() {
    println!("basic!");
}

fn read_a_system(a: Read<A>) {
    println!("{:?}", a)
}

fn write_a_system(mut a: Write<A>) {
    a.0 += 1;
    println!("{:?}", a)
}

fn read_write_many(mut a: Write<A>, b: Read<B>, c: Write<C>, d: Read<D>, e: Read<E>) {
    a.0 += b.0;
    println!("{a:?}, {b:?}, {c:?}, {d:?}, {e:?}")
}
