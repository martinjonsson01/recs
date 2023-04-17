use bevy_ecs::prelude::*;
use bevy_tasks::{ComputeTaskPool, TaskPoolBuilder};
use cgmath::*;

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Velocity(Vector3<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Affine(Matrix4<f32>);

pub struct Benchmark(World);

impl Benchmark {
    pub fn new() -> Self {
        let mut world = World::default();

        world.spawn_batch((0..1000).map(|_| {
            (
                Affine(Matrix4::<f32>::from_angle_x(Rad(1.2))),
                Position(Vector3::unit_x()),
                Rotation(Vector3::unit_x()),
                Velocity(Vector3::unit_x()),
            )
        }));

        ComputeTaskPool::init(|| TaskPoolBuilder::new().build());

        Self(world)
    }

    pub fn run(&mut self) {
        let mut query = self.0.query::<(&mut Position, &mut Affine)>();

        let mut par_iter = query.par_iter(&self.0);
        par_iter.for_each_mut(|(mut pos, mut aff)| {
            for _ in 0..100 {
                aff.0 = aff.0.invert().unwrap();
            }

            pos.0 = aff.0.transform_vector(pos.0);
        });
    }
}
