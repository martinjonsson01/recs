use crossbeam::channel::unbounded;
use ecs::pool::ThreadPool;
use ecs::{Application, Read, Write};
use scheduler::schedule_dag::PrecedenceGraph;
use std::thread;
use std::time::Duration;

fn main() {
    use tracing_subscriber::layer::SubscriberExt;
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(tracing_tracy::TracyLayer::new()),
    )
    .unwrap();

    let mut application: Application = Application::default()
        .add_system(basic_system)
        .add_system(system_with_parameter)
        .add_system(system_with_two_parameters)
        .add_system(system_with_two_mutable_parameters)
        .add_system(system_with_read_and_write)
        .add_system(system_with_two_parameters);

    for k in 0..1 {
        let entity = application.new_entity();
        application.add_component_to_entity(entity, A);
        application.add_component_to_entity(entity, B);
        application.add_component_to_entity(
            entity,
            Position {
                x: 100.0 - (k as f32),
                y: k as f32,
            },
        );
    }

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    application.run::<ThreadPool, PrecedenceGraph>(shutdown_receiver)
}

#[derive(Debug, Default)]
pub struct A;
#[derive(Debug, Default)]
pub struct B;

#[derive(Debug, Copy, Clone, PartialEq)]
struct Position {
    x: f32,
    y: f32,
}

#[tracing::instrument]
fn basic_system() {
    let _span = tracing_tracy::client::span!("basic_system");
    thread::sleep(Duration::from_millis(100));
}

#[tracing::instrument]
fn system_with_parameter(_: Read<A>) {
    let _span = tracing_tracy::client::span!("system_with_parameter");
    thread::sleep(Duration::from_millis(100));
}

#[tracing::instrument]
fn system_with_two_parameters(_: Read<A>, _: Read<B>) {
    let _span = tracing_tracy::client::span!("system_with_two_parameters");
    thread::sleep(Duration::from_millis(100));
}

#[tracing::instrument]
fn system_with_two_mutable_parameters(_: Write<A>, _: Write<B>) {
    let _span = tracing_tracy::client::span!("system_with_two_mutable_parameters");
    thread::sleep(Duration::from_millis(100));
}

#[tracing::instrument]
fn system_with_read_and_write(_: Read<A>, _: Write<B>) {
    let _span = tracing_tracy::client::span!("system_with_read_and_write");
    thread::sleep(Duration::from_millis(100));
}
