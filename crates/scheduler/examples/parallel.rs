use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::profiling::Profileable;
use ecs::systems::{Read, Write};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use tracing::{error, info, instrument, warn};

// a simple example of how to use the crate `ecs`
#[instrument]
fn main() -> Result<(), Report> {
    let mut app = BasicApplicationBuilder::default()
        .with_profiling()?
        .add_system(basic_system)
        .add_system(read_b_system)
        .add_system(write_b_system)
        .add_system(read_write_many)
        .build();

    app.create_entities_with(200_000, |i| (A(i), B(i), C, D, E))
        .unwrap();

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

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
#[cfg_attr(feature = "profile", inline(never))]
fn basic_system() {
    info!("no parameters!");
}

#[instrument]
#[cfg_attr(feature = "profile", inline(never))]
fn read_b_system(b: Read<B>) {
    if b.0 % 1000 == 0 {
        info!("executing");
    }
    if b.0 > 100_000 {
        warn!("{b:?} is very big!")
    }
    if b.0 == i32::MAX {
        error!("{b:?} is way too big!")
    }
}

#[instrument]
#[cfg_attr(feature = "profile", inline(never))]
fn write_b_system(mut b: Write<B>) {
    b.0 += 1;
    if b.0 % 1000 == 0 {
        info!("executing");
    }
}

#[instrument]
#[cfg_attr(feature = "profile", inline(never))]
fn read_write_many(mut a: Write<A>, b: Read<B>, _: Write<C>, _: Read<D>, _: Read<E>) {
    a.0 += b.0;
    if a.0 % 1000 == 0 {
        info!("executing");
    }
}
