use cgmath::*;
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

            for _ in 0..10_000 {
                let entity = app.create_entity().unwrap();
                app.add_component(entity, Matrix4::from_scale(1.0)).unwrap();
                app.add_component(entity, Position(Vector3::unit_x()))
                    .unwrap();
                app.add_component(entity, Rotation(Vector3::unit_x()))
                    .unwrap();
                app.add_component(entity, Velocity(Vector3::unit_x()))
                    .unwrap();
            }

            app
        });

        Self
    }

    pub fn run(&mut self) {
        todo!("implement a Query::new() that works")
        /*let app = APPLICATION.get().unwrap();

        let system = (|| {}).into_system();
        let query: Query<(Write<Position>, Read<Velocity>)> = Query::new(&app.world, &system);

        let query_iterator = query.try_into_iter().unwrap();
        for (mut position, velocity) in query_iterator {
            position.0 += velocity.0;
        }*/
    }
}
