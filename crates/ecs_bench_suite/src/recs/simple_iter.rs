use cgmath::*;
use ecs::systems::{IntoSystem, Query, Read, Write};
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

        let system = (|| {}).into_system();
        let query: Query<(Write<Position>, Read<Velocity>)> = Query::new(&app.world, &system);

        let query_iterator = query.try_into_iter().unwrap();
        for (mut position, velocity) in query_iterator {
            position.0 += velocity.0;
        }
    }
}
