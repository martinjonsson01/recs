use crossbeam::channel::{unbounded, Sender};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use n_body::scenes::{all_heavy_random_cube_with_bodies, create_planet_entity};
use n_body::{acceleration, gravity, movement, BodySpawner};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::time::{Duration, SystemTime};

#[derive(Clone)]
pub struct Benchmark {
    body_count: u32,
}

static mut SHUTDOWN_SENDER: Option<Sender<()>> = None;
static mut TICKS: u64 = 0;
static mut START_TIME: SystemTime = SystemTime::UNIX_EPOCH;
static mut SIMULATED_TICKS: u64 = 1;
static mut DURATION: Duration = Duration::ZERO;

fn benchmark_system() {
    unsafe {
        if TICKS == 0 {
            START_TIME = SystemTime::now();
        } else if TICKS == SIMULATED_TICKS {
            DURATION = SystemTime::elapsed(&START_TIME).unwrap();

            drop(SHUTDOWN_SENDER.take());
        }
        TICKS += 1;
    }
}

impl Benchmark {
    pub fn new(body_count: u32) -> Self {
        Self { body_count }
    }

    pub fn run(&mut self, target_tick_count: u64) -> Duration {
        let Self { body_count } = self.clone();

        unsafe {
            TICKS = 0;
            START_TIME = SystemTime::UNIX_EPOCH;
            SIMULATED_TICKS = target_tick_count;
            DURATION = Duration::ZERO;
        }

        let mut app = BasicApplicationBuilder::default()
            .add_system(movement)
            .add_system(acceleration)
            .add_system(gravity)
            .add_system(benchmark_system)
            .build();

        all_heavy_random_cube_with_bodies(body_count)
            .spawn_bodies(&mut app, create_planet_entity)
            .unwrap();

        let (shutdown_sender, shutdown_receiver) = unbounded();
        unsafe {
            SHUTDOWN_SENDER = Some(shutdown_sender);
        }

        app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)
            .unwrap();

        unsafe { DURATION }
    }
}
