use crossbeam::channel::Sender;
use ecs::systems::Write;
use ecs::{Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder, Schedule};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::sync::OnceLock;

#[derive(Debug)]
struct A(f32);

#[derive(Debug)]
struct B(f32);

#[derive(Debug)]
struct C(f32);

#[derive(Debug)]
struct D(f32);

#[derive(Debug)]
struct E(f32);

fn ab(mut a: Write<A>, mut b: Write<B>) {
    std::mem::swap(&mut a.0, &mut b.0);
}

fn cd(mut c: Write<C>, mut d: Write<D>) {
    std::mem::swap(&mut c.0, &mut d.0);
}

fn ce(mut c: Write<C>, mut e: Write<E>) {
    std::mem::swap(&mut c.0, &mut e.0);
}

static APPLICATION: OnceLock<BasicApplication> = OnceLock::new();
static WORKER_SHUTDOWN_SENDER: OnceLock<Sender<()>> = OnceLock::new();

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        APPLICATION.get_or_init(|| {
            let mut app = BasicApplicationBuilder::default()
                .add_system(ab)
                .add_system(cd)
                .add_system(ce)
                .build();

            for _ in 0..10000 {
                let entity = app.create_entity().unwrap();
                app.add_component(entity, A(0.0)).unwrap();
                app.add_component(entity, B(0.0)).unwrap();
            }

            for _ in 0..10000 {
                let entity = app.create_entity().unwrap();
                app.add_component(entity, A(0.0)).unwrap();
                app.add_component(entity, B(0.0)).unwrap();
                app.add_component(entity, C(0.0)).unwrap();
            }

            for _ in 0..10000 {
                let entity = app.create_entity().unwrap();
                app.add_component(entity, A(0.0)).unwrap();
                app.add_component(entity, B(0.0)).unwrap();
                app.add_component(entity, C(0.0)).unwrap();
                app.add_component(entity, D(0.0)).unwrap();
            }

            for _ in 0..10000 {
                let entity = app.create_entity().unwrap();
                app.add_component(entity, A(0.0)).unwrap();
                app.add_component(entity, B(0.0)).unwrap();
                app.add_component(entity, C(0.0)).unwrap();
                app.add_component(entity, E(0.0)).unwrap();
            }

            let worker_shutdown_sender = WorkerPool::initialize_global();

            WORKER_SHUTDOWN_SENDER.set(worker_shutdown_sender).unwrap();

            app
        });

        Self
    }

    pub fn run(&mut self) {
        let app = APPLICATION.get().unwrap();

        let mut schedule = PrecedenceGraph::generate(&app.systems).unwrap();
        WorkerPool::execute_tick(&mut schedule, &app.world).unwrap();
    }
}
