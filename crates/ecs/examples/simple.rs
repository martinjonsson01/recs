use ecs::{Application, Read, ReadWrite, Sequential, Unordered};

// a simple example of how to use the crate `ecs`
fn main() {
    let app = Application::default()
        .add_system(basic_system)
        .add_system(read_a_system)
        .add_system(write_a_system)
        .add_system(read_write_many);

    for _ in 0..10 {
        let entity = app.create_entity();
        app.add_component(entity, A);
    }

    app.run::<Sequential, Unordered>();
}

#[derive(Debug)]
struct A;

#[derive(Debug)]
struct B;

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

fn write_a_system(mut a: ReadWrite<A>) {
    println!("{:?}", a)
}

fn read_write_many(mut a: ReadWrite<A>, b: Read<B>, mut c: ReadWrite<C>, d: Read<D>, e: Read<E>) {
    println!("{a:?}, {b:?}, {c:?}, {d:?}, {e:?}")
}
