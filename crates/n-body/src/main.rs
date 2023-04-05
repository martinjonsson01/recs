use cgmath::{InnerSpace, MetricSpace, Point3, Vector3, Zero};
use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::logging::Loggable;
use ecs::systems::{Query, Read, Write};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use gfx_plugin::rendering::{PointLight, Position};
use gfx_plugin::{Graphical, GraphicalApplication};
use rand::distributions::Standard;
use rand::prelude::Distribution;
use rand::Rng;
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::num::NonZeroU32;
use tracing::instrument;

// a simple example of how to use the crate `ecs`
#[instrument]
fn main() -> GenericResult<()> {
    let mut app = BasicApplicationBuilder::default()
        .with_rendering()?
        .with_tracing()?
        .add_system(movement)
        .add_system(acceleration)
        .add_system(gravity)
        .build()?;

    app.spawn_bodies(NonZeroU32::new(BODY_COUNT).expect("body count should be non-zero"))?;

    app.spawn_sun()?;

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}

// Assuming a tickrate of 500 ticks per second.
const ASSUMED_TICK_DELTA_SECONDS: f32 = 1.0 / 237.0;

const BODY_COUNT: u32 = 1000;
const INITIAL_POSITION_MIN: f32 = -INITIAL_POSITION_MAX;
const INITIAL_POSITION_MAX: f32 = 100.0;
const MINIMUM_MASS: f32 = 1_000.0;
const MAXIMUM_MASS: f32 = 10_000.0;
const SUN_MASS: f32 = 1_000_000_000.0;
const INITIAL_VELOCITY_MIN: f32 = 0.0;
const INITIAL_VELOCITY_MAX: f32 = 0.01;
const INITIAL_ACCELERATION_MIN: f32 = -INITIAL_ACCELERATION_MAX;
const INITIAL_ACCELERATION_MAX: f32 = 1.0;

// Need a wrapper because the trait can't be implemented on foreign types.
struct RandomPosition(Position);
impl Distribution<RandomPosition> for Standard {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> RandomPosition {
        RandomPosition(Position {
            point: [
                random.gen_range(INITIAL_POSITION_MIN..INITIAL_POSITION_MAX),
                random.gen_range(INITIAL_POSITION_MIN..INITIAL_POSITION_MAX),
                random.gen_range(INITIAL_POSITION_MIN..INITIAL_POSITION_MAX),
            ]
            .into(),
        })
    }
}

/// The mass (in kilograms) of a body.
#[derive(Debug)]
struct Mass(f32);

impl Distribution<Mass> for Standard {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Mass {
        Mass(random.gen_range(MINIMUM_MASS..MAXIMUM_MASS))
    }
}

/// How fast a body is moving, in meters/second.
#[derive(Debug)]
struct Velocity(Vector3<f32>);

impl Distribution<Velocity> for Standard {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Velocity {
        Velocity(
            [
                random.gen_range(INITIAL_VELOCITY_MIN..INITIAL_VELOCITY_MAX),
                random.gen_range(INITIAL_VELOCITY_MIN..INITIAL_VELOCITY_MAX),
                random.gen_range(INITIAL_VELOCITY_MIN..INITIAL_VELOCITY_MAX),
            ]
            .into(),
        )
    }
}

/// How fast a body is accelerating, in meters/second^2.
#[derive(Debug)]
struct Acceleration(Vector3<f32>);

impl Distribution<Acceleration> for Standard {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Acceleration {
        Acceleration(
            [
                random.gen_range(INITIAL_ACCELERATION_MIN..INITIAL_ACCELERATION_MAX),
                random.gen_range(INITIAL_ACCELERATION_MIN..INITIAL_ACCELERATION_MAX),
                random.gen_range(INITIAL_ACCELERATION_MIN..INITIAL_ACCELERATION_MAX),
            ]
            .into(),
        )
    }
}

type GenericResult<T> = Result<T, Report>;

trait NBodyApplication {
    fn spawn_bodies(&mut self, body_count: NonZeroU32) -> GenericResult<()>;
    fn spawn_sun(&mut self) -> GenericResult<()>;
}
impl<InnerApp: Application + Send + Sync> NBodyApplication for GraphicalApplication<InnerApp> {
    fn spawn_bodies(&mut self, body_count: NonZeroU32) -> GenericResult<()> {
        let body_model = self.load_model("cube.obj")?;
        let mut random = rand::thread_rng();

        for _ in 0..body_count.get() {
            let RandomPosition(position): RandomPosition = random.gen();
            let mass: Mass = random.gen();
            let velocity: Velocity = random.gen();
            let acceleration: Acceleration = random.gen();

            let entity = self
                .rendered_entity_builder(body_model)?
                .with_position(position)
                .build()?;

            self.add_component(entity, mass)?;
            self.add_component(entity, velocity)?;
            self.add_component(entity, acceleration)?;
        }

        Ok(())
    }

    fn spawn_sun(&mut self) -> GenericResult<()> {
        let mut random = rand::thread_rng();

        let light_source = self.create_entity()?;
        self.add_component(
            light_source,
            PointLight {
                color: [
                    random.gen_range(0.0..1.0),
                    random.gen_range(0.0..1.0),
                    random.gen_range(0.0..1.0),
                ]
                .into(),
            },
        )?;

        let position = Position::default();
        self.add_component(light_source, position)?;
        self.add_component(light_source, Mass(SUN_MASS))?;
        self.add_component(light_source, Velocity(Vector3::zero()))?;
        self.add_component(light_source, Acceleration(Vector3::zero()))?;

        Ok(())
    }
}

fn movement(mut position: Write<Position>, velocity: Read<Velocity>) {
    let Velocity(velocity) = *velocity;

    position.point += velocity * ASSUMED_TICK_DELTA_SECONDS;
}

fn acceleration(mut velocity: Write<Velocity>, acceleration: Read<Acceleration>) {
    let Velocity(ref mut velocity) = *velocity;
    let Acceleration(acceleration) = *acceleration;

    *velocity += acceleration * ASSUMED_TICK_DELTA_SECONDS;
}

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
