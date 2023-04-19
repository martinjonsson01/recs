use cgmath::*;
use crossbeam::channel::Sender;
use ecs::systems::{IntoSystem, System, Write};
use ecs::{
    Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder,
    SystemExecutionGuard,
};
use scheduler::executor::WorkerPool;
use std::sync::OnceLock;

#[derive(Copy, Clone, Debug)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Velocity(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Affine(Matrix4<f32>);

#[derive(Debug)]
struct InnerBenchmark(BasicApplication, Sender<()>);

// These two need to be stored as statics, in order to be able to get 'static lifetimes from
// them. If they're stored inside `Benchmark` the references won't live long enough for
// the workerpool to execute on them.
static BENCHMARK_DATA: OnceLock<InnerBenchmark> = OnceLock::new();
static HEAVY_COMPUTATION_SYSTEM: OnceLock<Box<dyn System>> = OnceLock::new();

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        HEAVY_COMPUTATION_SYSTEM.get_or_init(|| {
            let heavy_system = heavy_computation_system.into_system();
            Box::new(heavy_system)
        });

        BENCHMARK_DATA.get_or_init(|| {
            let mut app = BasicApplicationBuilder::default().build();

            for _ in 0..1000 {
                let entity = app.create_entity().unwrap();
                app.add_component(entity, Affine(Matrix4::<f32>::from_angle_x(Rad(1.2))))
                    .unwrap();
                app.add_component(entity, Position(Vector3::unit_x()))
                    .unwrap();
                app.add_component(entity, Rotation(Vector3::unit_x()))
                    .unwrap();
                app.add_component(entity, Velocity(Vector3::unit_x()))
                    .unwrap();
            }

            let worker_shutdown_sender = WorkerPool::initialize_global();

            InnerBenchmark(app, worker_shutdown_sender)
        });

        Self
    }

    pub fn run(&mut self) {
        let benchmark_data = BENCHMARK_DATA.get().unwrap();
        let InnerBenchmark(app, _) = benchmark_data;
        let heavy_computation_system = HEAVY_COMPUTATION_SYSTEM.get().unwrap();

        let (system_execution_guard, system_completion_receiver) =
            SystemExecutionGuard::create(heavy_computation_system.as_ref());

        let tasks = scheduler::executor::create_system_task(system_execution_guard, &app.world);
        WorkerPool::dispatch_tasks(tasks);

        let _ = system_completion_receiver.recv();
    }
}

fn heavy_computation_system(mut position: Write<Position>, mut affine: Write<Affine>) {
    for _ in 0..100 {
        affine.0 = affine.0.invert().unwrap();
    }
    position.0 = affine.0.transform_vector(position.0);
}
