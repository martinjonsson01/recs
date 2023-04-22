use crossbeam::channel::Sender;
use ecs::systems::{IntoSystem, System};
use ecs::{ApplicationBuilder, BasicApplication, BasicApplicationBuilder, Schedule};
use n_body::scenes::{create_planet_entity, ALL_HEAVY_RANDOM_CUBE};
use n_body::{acceleration, gravity, movement, BodySpawner};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::sync::{Mutex, OnceLock};

#[derive(Debug)]
struct Data(f32);

static APPLICATION: OnceLock<BasicApplication> = OnceLock::new();
static SCHEDULE: Mutex<Option<PrecedenceGraph>> = Mutex::new(None);
static SYSTEMS: OnceLock<Vec<Box<dyn System>>> = OnceLock::new();
static GLOBAL_WORKER_POOL: OnceLock<Sender<()>> = OnceLock::new();

pub struct Benchmark;

impl Default for Benchmark {
    fn default() -> Self {
        SYSTEMS.get_or_init(|| {
            let movement_system: Box<dyn System> = Box::new(movement.into_system());
            let acceleration_system: Box<dyn System> = Box::new(acceleration.into_system());
            let gravity_system: Box<dyn System> = Box::new(gravity.into_system());
            vec![movement_system, acceleration_system, gravity_system]
        });

        GLOBAL_WORKER_POOL.get_or_init(WorkerPool::initialize_global);

        APPLICATION.get_or_init(|| {
            let mut app = BasicApplicationBuilder::default().build();

            ALL_HEAVY_RANDOM_CUBE
                .spawn_bodies(&mut app, create_planet_entity)
                .unwrap();

            let systems = SYSTEMS.get().unwrap();
            let schedule = PrecedenceGraph::generate(systems).unwrap();
            let mut schedule_guard = SCHEDULE.lock().unwrap();
            *schedule_guard = Some(schedule);

            app
        });

        Self
    }
}

impl Benchmark {
    pub fn run(&mut self) {
        let app = APPLICATION.get().unwrap();
        let mut schedule_guard = SCHEDULE.lock().unwrap();
        let schedule = schedule_guard.as_mut().unwrap();

        WorkerPool::execute_tick(schedule, &app.world).unwrap();
    }
}
