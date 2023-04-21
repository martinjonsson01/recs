use cgmath::*;

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Transform(Matrix4<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, bevy_ecs::component::Component)]
struct Velocity(Vector3<f32>);

#[derive(Default)]
struct NaiveVecs {
    positions: Vec<Position>,
    velocities: Vec<Velocity>,
}

pub struct Benchmark(NaiveVecs);

impl Benchmark {
    pub fn new() -> Self {
        let mut naive_vecs = NaiveVecs::default();

        for _ in 0..10_000 {
            naive_vecs.positions.push(Position(Vector3::unit_x()));
            naive_vecs.velocities.push(Velocity(Vector3::unit_x()));
        }

        Self(naive_vecs)
    }

    pub fn run(&mut self) {
        let NaiveVecs {
            positions,
            velocities,
        } = &mut self.0;

        for (velocity, position) in velocities.iter().zip(positions.iter_mut()) {
            position.0 += velocity.0;
        }
    }
}
