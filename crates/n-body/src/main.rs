#[allow(unused)] // When swapping between scenes some will always go unused.
mod scenes;

use crate::scenes::*;
use cgmath::{Deg, InnerSpace, MetricSpace, Point3, Vector3, Zero};
use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::systems::{Query, Read, Write};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use gfx_plugin::rendering::Position;
use gfx_plugin::Graphical;
use rand::distributions::Uniform;
use rand::prelude::Distribution;
use rand::Rng;
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use tracing::instrument;

#[instrument]
fn main() -> GenericResult<()> {
    let mut app_builder = BasicApplicationBuilder::default().with_rendering()?;

    #[cfg(feature = "profile")]
    {
        use ecs::profiling::Profileable;
        app_builder = app_builder.with_profiling()?;
    }
    #[cfg(not(feature = "profile"))]
    {
        use ecs::logging::Loggable;
        app_builder = app_builder.with_tracing()?;
    }

    let mut app = app_builder
        .output_directory(env!("OUT_DIR"))
        .light_model("sphere.obj")
        .field_of_view(Deg(90.0))
        .far_clipping_plane(10_000.0)
        .camera_movement_speed(100.0)
        .add_system(movement)
        .add_system(acceleration)
        .add_system(gravity)
        .build()?;

    UNEVEN_WEIGHTS_RANDOM_CUBE.spawn_bodies(&mut app, create_rendered_planet_entity)?;
    SINGLE_HEAVY_BODY_AT_ORIGIN.spawn_bodies(&mut app, create_rendered_sun_entity)?;

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}

// todo(#90): change to use dynamic delta time.
// todo(#90) currently assuming a hardcoded tick rate.
const FIXED_TIME_STEP: f32 = 1.0 / 200.0;

trait BodySpawner {
    fn spawn_bodies<App, CreateEntityFn>(
        &self,
        app: &mut App,
        create_entity: CreateEntityFn,
    ) -> GenericResult<()>
    where
        App: Application,
        CreateEntityFn: Fn(&mut App, Position, Mass, Velocity, Acceleration) -> GenericResult<()>;
}

// Need a wrapper because the trait can't be implemented on foreign types.
struct RandomPosition(Position);
impl Distribution<RandomPosition> for Uniform<f32> {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> RandomPosition {
        RandomPosition(Position {
            point: [
                self.sample(random),
                self.sample(random),
                self.sample(random),
            ]
            .into(),
        })
    }
}

/// The mass (in kilograms) of a body.
#[derive(Debug)]
struct Mass(f32);

impl Distribution<Mass> for Uniform<f32> {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Mass {
        Mass(self.sample(random))
    }
}

/// How fast a body is moving, in meters/second.
#[derive(Debug)]
struct Velocity(Vector3<f32>);

impl Distribution<Velocity> for Uniform<f32> {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Velocity {
        Velocity(
            [
                self.sample(random),
                self.sample(random),
                self.sample(random),
            ]
            .into(),
        )
    }
}

/// How fast a body is accelerating, in meters/second^2.
#[derive(Debug)]
struct Acceleration(Vector3<f32>);

impl Distribution<Acceleration> for Uniform<f32> {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Acceleration {
        Acceleration(
            [
                self.sample(random),
                self.sample(random),
                self.sample(random),
            ]
            .into(),
        )
    }
}

type GenericResult<T> = Result<T, Report>;

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
fn movement(mut position: Write<Position>, velocity: Read<Velocity>) {
    let Velocity(velocity) = *velocity;

    position.point += velocity * FIXED_TIME_STEP;
}

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
fn acceleration(mut velocity: Write<Velocity>, acceleration: Read<Acceleration>) {
    let Velocity(ref mut velocity) = *velocity;
    let Acceleration(acceleration) = *acceleration;

    *velocity += acceleration * FIXED_TIME_STEP;
}

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
fn gravity(
    position: Read<Position>,
    mut acceleration: Write<Acceleration>,
    mass: Read<Mass>,
    bodies_query: Query<(Read<Position>, Read<Mass>)>,
) {
    let Position { point: position } = *position;
    let Mass(mass) = *mass;
    let Acceleration(ref mut acceleration) = *acceleration;

    let acceleration_towards_body = |(body_position, body_mass): (Point3<f32>, f32)| {
        let to_body = body_position - position;
        let distance_squared = to_body.distance2(Vector3::zero());

        if distance_squared <= f32::EPSILON {
            return Vector3::zero();
        }

        // Newton's law of universal gravitation.
        const GRAVITATIONAL_CONSTANT: f32 = 0.00000000006674;
        let force = GRAVITATIONAL_CONSTANT * ((mass * body_mass) / distance_squared);

        to_body.normalize() * force
    };

    let total_acceleration: Vector3<f32> = bodies_query
        .into_iter()
        .map(|(position, mass)| (position.point, mass.0))
        .map(acceleration_towards_body)
        .sum();
    *acceleration = total_acceleration;
}
