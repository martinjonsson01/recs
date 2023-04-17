use cgmath::*;
use ecs::systems::iteration::SequentiallyIterable;
use ecs::systems::{IntoSystem, Write};
use ecs::{Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder};

#[derive(Copy, Clone, Debug)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Velocity(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Affine(Matrix4<f32>);

pub struct Benchmark(BasicApplication);

impl Benchmark {
    pub fn new() -> Self {
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

        Self(app)
    }

    pub fn run(&mut self) {
        let Benchmark(app) = self;

        let heavy_computation_system =
            (|mut position: Write<Position>, mut affine: Write<Affine>| {
                for _ in 0..100 {
                    affine.0 = affine.0.invert().unwrap();
                }

                position.0 = affine.0.transform_vector(position.0);
            })
            .into_system();

        heavy_computation_system.run(&app.world).unwrap();
    }
}
