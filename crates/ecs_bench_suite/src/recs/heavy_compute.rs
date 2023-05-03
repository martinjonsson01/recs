use cgmath::*;
use ecs::systems::Write;
use ecs::{
    Application, ApplicationBuilder, ApplicationRunner, BasicApplicationBuilder, IntoTickable,
    Tickable,
};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;

#[derive(Copy, Clone, Debug)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Velocity(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Affine(Matrix4<f32>);

pub struct Benchmark(ApplicationRunner<WorkerPool, PrecedenceGraph>);

impl Benchmark {
    pub fn new() -> Self {
        let mut app = BasicApplicationBuilder::default()
            .add_system(heavy_computation_system)
            .build();

        for _ in 0..1000 {
            app.create_entity((
                Affine(Matrix4::<f32>::from_angle_x(Rad(1.2))),
                Position(Vector3::unit_x()),
                Rotation(Vector3::unit_x()),
                Velocity(Vector3::unit_x()),
            ))
            .unwrap();
        }

        Self(app.into_tickable().unwrap())
    }

    pub fn run(&mut self) {
        self.0.tick().unwrap();
    }
}
fn heavy_computation_system(mut position: Write<Position>, mut affine: Write<Affine>) {
    for _ in 0..100 {
        affine.0 = affine.0.invert().unwrap();
    }
    position.0 = affine.0.transform_vector(position.0);
}
