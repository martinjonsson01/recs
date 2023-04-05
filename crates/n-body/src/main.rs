use cgmath::{Vector3, Zero};
use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::logging::Loggable;
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
        .build()?;

    app.spawn_bodies(NonZeroU32::new(BODY_COUNT).expect("body count should be non-zero"))?;

    app.spawn_sun()?;

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}

const BODY_COUNT: u32 = 1000;
const INITIAL_POSITION_MIN: f32 = -100.0;
const INITIAL_POSITION_MAX: f32 = 100.0;
const MINIMUM_MASS: u32 = 10;
const MAXIMUM_MASS: u32 = 10_000;
const SUN_MASS: u32 = 100_000;
const INITIAL_VELOCITY_MIN: f32 = 0.0;
const INITIAL_VELOCITY_MAX: f32 = 100.0;
const INITIAL_ACCELERATION_MIN: f32 = 0.0;
const INITIAL_ACCELERATION_MAX: f32 = 100.0;

// Need a wrapper because the trait can't be implemented on foreign types.
struct RandomPosition(Position);
impl Distribution<RandomPosition> for Standard {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> RandomPosition {
        RandomPosition(Position {
            vector: [
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
struct Mass(u32);

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
        self.add_component(
            light_source,
            Position {
                vector: Vector3::zero(),
            },
        )?;

        self.add_component(light_source, Mass(SUN_MASS))?;
        self.add_component(light_source, Velocity(Vector3::zero()))?;
        self.add_component(light_source, Acceleration(Vector3::zero()))?;

        Ok(())
    }
}
