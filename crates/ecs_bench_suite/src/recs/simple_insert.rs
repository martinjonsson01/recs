use cgmath::*;
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};

#[derive(Copy, Clone, Debug)]
struct Transform(Matrix4<f32>);

#[derive(Copy, Clone, Debug)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Velocity(Vector3<f32>);

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&mut self) {
        let mut app = BasicApplicationBuilder::default().build();

        for _ in 0..10_000 {
            let entity = app.create_empty_entity().unwrap();
            app.add_component(entity, Matrix4::from_scale(1.0)).unwrap();
            app.add_component(entity, Position(Vector3::unit_x()))
                .unwrap();
            app.add_component(entity, Rotation(Vector3::unit_x()))
                .unwrap();
            app.add_component(entity, Velocity(Vector3::unit_x()))
                .unwrap();
        }
    }
}
