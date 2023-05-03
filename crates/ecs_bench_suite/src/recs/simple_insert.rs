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
            app.create_entity((
                Matrix4::from_scale(1.0),
                Position(Vector3::unit_x()),
                Rotation(Vector3::unit_x()),
                Velocity(Vector3::unit_x()),
            ))
            .unwrap();
        }
    }
}
