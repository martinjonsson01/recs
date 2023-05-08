use cgmath::*;
use ecs::systems::iteration::SequentiallyIterable;
use ecs::systems::{IntoSystem, Read, Write};
use ecs::{Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder};
use std::sync::OnceLock;

#[derive(Copy, Clone, Debug)]
struct Transform(Matrix4<f32>);

#[derive(Copy, Clone, Debug)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Velocity(Vector3<f32>);

static APPLICATION: OnceLock<BasicApplication> = OnceLock::new();

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        APPLICATION.get_or_init(|| {
            let mut app = BasicApplicationBuilder::default().build();

            app.create_entities_with(10_000, |_| {
                (
                    Matrix4::from_scale(1.0),
                    Position(Vector3::unit_x()),
                    Rotation(Vector3::unit_x()),
                    Velocity(Vector3::unit_x()),
                )
            })
            .unwrap();

            app
        });

        Self
    }

    pub fn run(&mut self) {
        let app = APPLICATION.get().unwrap();

        let movement_system = |mut position: Write<Position>, velocity: Read<Velocity>| {
            position.0 += velocity.0;
        };
        let movement_system = movement_system.into_system();

        movement_system.run(&app.world).unwrap();
    }
}
