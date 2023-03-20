use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::{Application, Read, Sequential, Write};
use scheduler::schedule::PrecedenceGraph;
use std::thread;
use std::time::Duration;
use tracing::{error, info, instrument, trace, warn};

// a simple example of how to use the crate `ecs`
#[instrument]
fn main() -> Result<(), Report> {
    let mut app = Application::default()
        .with_tracing()?
        .add_system(basic_system)
        .add_system(read_b_system)
        .add_system(write_b_system)
        .add_system(read_write_many);

    for i in 0..20 {
        let entity = app.create_entity();
        app.add_component(entity, A(i));
        app.add_component(entity, B(i));
        app.add_component(entity, C);
        app.add_component(entity, D);
        app.add_component(entity, E);
    }

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<Sequential, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
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

#[instrument]
fn basic_system() {
    trace!("very detailed message that is not normally shown");
    info!("no parameters!");
    thread::sleep(Duration::from_nanos(10));
}

#[instrument]
fn read_b_system(b: Read<B>) {
    info!("executing");
    if b.0 > 100_000 {
        warn!("{b:?} is very big!")
    }
    if b.0 == i32::MAX {
        error!("{b:?} is way too big!")
    }
    thread::sleep(Duration::from_nanos(10));
}

#[instrument]
fn write_b_system(mut b: Write<B>) {
    b.0 += 1;
    info!("executing");
    thread::sleep(Duration::from_nanos(10));
}

#[instrument]
fn read_write_many(mut a: Write<A>, b: Read<B>, _: Write<C>, _: Read<D>, _: Read<E>) {
    a.0 += b.0;
    info!("executing");
    thread::sleep(Duration::from_nanos(10));
}
