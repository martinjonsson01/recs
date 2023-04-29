use bevy::app::{AppExit, ScheduleRunnerSettings};
use bevy::prelude::*;
use crossbeam::sync::Parker;
use n_body::scenes::all_heavy_random_cube_with_bodies;
use n_body::{BodySpawner, FIXED_TIME_STEP};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

#[derive(Clone)]
pub struct Benchmark {
    body_count: u32,
    current_tick: Arc<AtomicU64>,
    start_time: Arc<Mutex<Option<Instant>>>,
}

impl Benchmark {
    pub fn new(body_count: u32) -> Self {
        Self {
            body_count,
            current_tick: Arc::new(AtomicU64::new(0)),
            start_time: Arc::new(Mutex::new(None)),
        }
    }

    pub fn run(&mut self, target_tick_count: u64) -> Duration {
        let Self {
            body_count,
            current_tick,
            start_time,
        } = self.clone();

        let mut app = App::new();

        let total_duration = Arc::new(Mutex::new(None));
        let total_duration_ref = Arc::clone(&total_duration);

        let shutdown_parker = Parker::new();
        let shutdown_unparker = shutdown_parker.unparker().clone();

        let benchmark_system = move |mut exit: EventWriter<AppExit>| {
            let mut start_time_guard = start_time.lock().unwrap();
            let start_time = start_time_guard.get_or_insert(Instant::now());

            if current_tick.load(Ordering::SeqCst) == target_tick_count {
                let mut total_duration_guard = total_duration_ref.lock().unwrap();
                *total_duration_guard = Some(start_time.elapsed());

                shutdown_unparker.unpark();
                exit.send(AppExit);
            } else {
                current_tick.fetch_add(1, Ordering::SeqCst);
            }
        };

        app.insert_resource(ScheduleRunnerSettings::run_loop(Duration::ZERO))
            .add_plugins(MinimalPlugins)
            .add_system(movement)
            .add_system(acceleration)
            .add_system(gravity)
            .add_system(benchmark_system);

        all_heavy_random_cube_with_bodies(body_count)
            .spawn_bodies(&mut app, create_bevy_planet_entity)
            .unwrap();

        app.run();

        shutdown_parker.park();

        let total_duration_guard = total_duration.lock().unwrap();
        total_duration_guard.unwrap()
    }
}

pub fn create_bevy_planet_entity(
    app: &mut App,
    position: n_body::Position,
    mass: n_body::Mass,
    velocity: n_body::Velocity,
    acceleration: n_body::Acceleration,
) -> n_body::GenericResult<()> {
    let transform = Transform::from_xyz(position.point.x, position.point.y, position.point.z);
    let mass: Mass = mass.into();
    let velocity: Velocity = velocity.into();
    let acceleration: Acceleration = acceleration.into();
    app.world.spawn((transform, mass, velocity, acceleration));

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
