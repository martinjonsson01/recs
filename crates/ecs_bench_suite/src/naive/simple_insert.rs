use cgmath::*;

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Transform(Matrix4<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Velocity(Vector3<f32>);

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&mut self) {
        let mut transforms = vec![];
        let mut positions = vec![];
        let mut rotations = vec![];
        let mut velocities = vec![];
        for _ in 0..10_000 {
            transforms.push(Transform(Matrix4::from_scale(1.0)));
            positions.push(Position(Vector3::unit_x()));
            rotations.push(Rotation(Vector3::unit_x()));
            velocities.push(Velocity(Vector3::unit_x()));
        }
    }
}
