use ecs::systems::{IntoSystem, System};
use ecs::{ApplicationBuilder, BasicApplication, BasicApplicationBuilder, Schedule};
use n_body::scenes::{all_heavy_random_cube_with_bodies, create_planet_entity};
use n_body::{acceleration, gravity, movement, BodySpawner};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::sync::OnceLock;

#[derive(Debug)]
struct Data(f32);

static SYSTEMS: OnceLock<Vec<Box<dyn System>>> = OnceLock::new();
static mut BENCHMARK_DATA: Option<BenchmarkData<'static>> = None;

struct BenchmarkData<'a>(BasicApplication, PrecedenceGraph<'a>);

pub struct Benchmark;

impl Benchmark {
    pub fn new(body_count: u32) -> Self {
        SYSTEMS.get_or_init(|| {
            let movement_system: Box<dyn System> = Box::new(movement.into_system());
            let acceleration_system: Box<dyn System> = Box::new(acceleration.into_system());
            let gravity_system: Box<dyn System> = Box::new(gravity.into_system());
            vec![movement_system, acceleration_system, gravity_system]
        });

        WorkerPool::initialize_global();

        let mut app = BasicApplicationBuilder::default().build();

        all_heavy_random_cube_with_bodies(body_count)
            .spawn_bodies(&mut app, create_planet_entity)
            .unwrap();

        let systems = SYSTEMS.get().unwrap();
        let schedule = PrecedenceGraph::generate(systems).unwrap();

        let data = BenchmarkData(app, schedule);

        // SAFETY: this benchmark will not access BENCHMARK_DATA concurrently.
        unsafe {
            BENCHMARK_DATA = Some(data);
        }

        Self
    }

    pub fn run(&mut self) {
        let BenchmarkData(app, schedule) =
            // SAFETY: this benchmark will not access BENCHMARK_DATA concurrently.
            unsafe { BENCHMARK_DATA.as_mut().unwrap() };

        WorkerPool::execute_tick(schedule, &app.world).unwrap();
    }
}
