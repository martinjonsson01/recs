use ecs::systems::{IntoSystem, System, Write};
use ecs::{Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder, Schedule};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::sync::{Mutex, OnceLock};

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
static SCHEDULE: Mutex<Option<PrecedenceGraph>> = Mutex::new(None);
static SYSTEMS: OnceLock<Vec<Box<dyn System>>> = OnceLock::new();

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        SYSTEMS.get_or_init(|| {
            let ab_system: Box<dyn System> = Box::new(ab.into_system());
            let cd_system: Box<dyn System> = Box::new(cd.into_system());
            let ce_system: Box<dyn System> = Box::new(ce.into_system());
            vec![ab_system, cd_system, ce_system]
        });

        WorkerPool::initialize_global();

        APPLICATION.get_or_init(|| {
            let mut app = BasicApplicationBuilder::default().build();

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

            let systems = SYSTEMS.get().unwrap();
            let schedule = PrecedenceGraph::generate(systems).unwrap();
            let mut schedule_guard = SCHEDULE.lock().unwrap();
            *schedule_guard = Some(schedule);

            app
        });

        Self
    }

    pub fn run(&mut self) {
        let app = APPLICATION.get().unwrap();
        let mut schedule_guard = SCHEDULE.lock().unwrap();
        let schedule = schedule_guard.as_mut().unwrap();

        WorkerPool::execute_tick(schedule, &app.world).unwrap();
    }
}
