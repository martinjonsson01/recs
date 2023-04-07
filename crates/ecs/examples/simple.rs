use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::logging::Loggable;
use ecs::systems::{Read, Write};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder, Sequential, Unordered};
use std::thread;
use std::time::Duration;
use tracing::{error, info, instrument, trace, warn};

// a simple example of how to use the crate `ecs`
#[instrument]
fn main() -> Result<(), Report> {
    let mut app = BasicApplicationBuilder::default()
        .with_tracing()?
        .add_system(basic_system)
        .add_system(read_a_system)
        .add_system(write_a_system)
        .add_system(read_write_many)
        .build();

    for i in 0..10 {
        let entity = app.create_entity()?;
        app.add_component(entity, A(i))?;
        app.add_component(entity, B(i))?;
        app.add_component(entity, C)?;
        app.add_component(entity, D)?;
        app.add_component(entity, E)?;
    }

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<Sequential, Unordered>(shutdown_receiver)?;

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
fn read_a_system(a: Read<A>) {
    info!("executing read_a_system");
    if a.0 > 100_000 {
        warn!("{a:?} is very big!")
    }
    if a.0 == i32::MAX {
        error!("{a:?} is way too big!")
    }
    thread::sleep(Duration::from_nanos(10));
}

#[instrument]
fn write_a_system(mut a: Write<A>) {
    a.0 += 1;
    info!("executing write_a_system");
    thread::sleep(Duration::from_nanos(10));
}

#[instrument]
fn read_write_many(mut a: Write<A>, b: Read<B>, _: Write<C>, _: Read<D>, _: Read<E>) {
    a.0 += b.0;
    info!("executing read_write_many");
    thread::sleep(Duration::from_nanos(10));
}
