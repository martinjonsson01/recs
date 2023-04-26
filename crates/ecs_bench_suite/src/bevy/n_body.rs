use bevy_ecs::{prelude::*, schedule::Schedule};
use bevy_transform::components::Transform;
use glam::Vec3;
use n_body::scenes::all_heavy_random_cube_with_bodies;
use n_body::{BodySpawner, FIXED_TIME_STEP};

pub struct Benchmark(World, Schedule);

/// The mass (in kilograms) of a body.
#[derive(Component)]
struct Mass(f32);

impl From<n_body::Mass> for Mass {
    fn from(value: n_body::Mass) -> Self {
        Self(value.0)
    }
}

/// How fast a body is moving, in meters/second.
#[derive(Component)]
struct Velocity(Vec3);

impl From<n_body::Velocity> for Velocity {
    fn from(value: n_body::Velocity) -> Self {
        Self(Vec3::new(value.0.x, value.0.y, value.0.z))
    }
}

/// How fast a body is accelerating, in meters/second^2.
#[derive(Component)]
struct Acceleration(Vec3);

impl From<n_body::Acceleration> for Acceleration {
    fn from(value: n_body::Acceleration) -> Self {
        Self(Vec3::new(value.0.x, value.0.y, value.0.z))
    }
}

impl Benchmark {
    pub fn new(body_count: u32) -> Self {
        let mut world = World::default();

        all_heavy_random_cube_with_bodies(body_count)
            .spawn_bodies(&mut world, create_bevy_planet_entity)
            .unwrap();

        let mut schedule = Schedule::default();
        schedule.add_system(movement);
        schedule.add_system(acceleration);
        schedule.add_system(gravity);

        Self(world, schedule)
    }

    pub fn run(&mut self) {
        self.1.run(&mut self.0);
    }
}

pub fn create_bevy_planet_entity(
    world: &mut World,
    position: n_body::Position,
    mass: n_body::Mass,
    velocity: n_body::Velocity,
    acceleration: n_body::Acceleration,
) -> n_body::GenericResult<()> {
    let mut entity = world.spawn_empty();

    let transform = Transform::from_xyz(position.point.x, position.point.y, position.point.z);
    let mass: Mass = mass.into();
    let velocity: Velocity = velocity.into();
    let acceleration: Acceleration = acceleration.into();
    entity.insert((transform, mass, velocity, acceleration));

    Ok(())
}

fn movement(mut query: Query<(&mut Transform, &Velocity)>) {
    query
        .par_iter_mut()
        .for_each_mut(|(mut transform, velocity)| {
            transform.translation += velocity.0 * FIXED_TIME_STEP;
        });
}

fn acceleration(mut query: Query<(&mut Velocity, &Acceleration)>) {
    query
        .par_iter_mut()
        .for_each_mut(|(mut velocity, acceleration)| {
            velocity.0 += acceleration.0 * FIXED_TIME_STEP;
        });
}

fn gravity(
    mut self_query: Query<(&Transform, &mut Acceleration)>,
    bodies_query: Query<(&Transform, &Mass)>,
) {
    self_query
        .par_iter_mut()
        .for_each_mut(|(transform, mut acceleration)| {
            let position = transform.translation;
            let acceleration_towards_body = |(body_position, body_mass): (Vec3, f32)| {
                let to_body: Vec3 = body_position - position;
                let distance_squared = to_body.length_squared();

                if distance_squared <= f32::EPSILON {
                    return Vec3::ZERO;
                }

                // Newton's law of universal gravitation.
                const GRAVITATIONAL_CONSTANT: f32 = 6.67e-11;
                let acceleration = GRAVITATIONAL_CONSTANT * body_mass / distance_squared;

                to_body.normalize() * acceleration
            };

            let total_acceleration: Vec3 = bodies_query
                .into_iter()
                .map(|(position, mass)| (position.translation, mass.0))
                .map(acceleration_towards_body)
                .sum();
            acceleration.0 = total_acceleration;
        });
}
