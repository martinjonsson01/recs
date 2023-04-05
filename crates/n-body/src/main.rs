use cgmath::{Deg, InnerSpace, MetricSpace, Point3, Vector3, Zero};
use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::logging::Loggable;
use ecs::systems::{Query, Read, Write};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use gfx_plugin::rendering::{PointLight, Position};
use gfx_plugin::{Graphical, GraphicalApplication};
use rand::distributions::Uniform;
use rand::prelude::Distribution;
use rand::Rng;
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use tracing::instrument;

#[instrument]
fn main() -> GenericResult<()> {
    let mut app = BasicApplicationBuilder::default()
        .with_rendering()?
        .with_tracing()?
        .field_of_view(Deg(90.0))
        .far_clipping_plane(10_000.0)
        .camera_movement_speed(100.0)
        .add_system(movement)
        .add_system(acceleration)
        .add_system(gravity)
        .build()?;

    let scene = LIGHT_BODIES_HEAVY_SUN;

    app.spawn_bodies(scene)?;
    app.spawn_sun(scene.sun_mass)?;

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}

// todo(#90): change to use dynamic delta time.
// todo(#90) currently assuming a hardcoded tick rate.
const ASSUMED_TICK_DELTA_SECONDS: f32 = 1.0 / 237.0;

const EVERYTHING_HEAVY: Scene = Scene {
    body_count: 1000,
    initial_position_min: -100.0,
    initial_position_max: 100.0,
    minimum_mass: 1_000.0,
    maximum_mass: 10_000_000.0,
    sun_mass: 1_000_000_000.0,
    initial_velocity_min: 0.0,
    initial_velocity_max: 0.01,
    initial_acceleration_min: -1.0,
    initial_acceleration_max: 1.0,
};

const LIGHT_BODIES_HEAVY_SUN: Scene = Scene {
    maximum_mass: 10_000.0,
    ..EVERYTHING_HEAVY
};

#[derive(Debug, Copy, Clone, PartialEq)]
struct Scene {
    body_count: u32,
    initial_position_min: f32,
    initial_position_max: f32,
    minimum_mass: f32,
    maximum_mass: f32,
    sun_mass: f32,
    initial_velocity_min: f32,
    initial_velocity_max: f32,
    initial_acceleration_min: f32,
    initial_acceleration_max: f32,
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

trait NBodyApplication {
    fn spawn_bodies(&mut self, scene: Scene) -> GenericResult<()>;
    fn spawn_sun(&mut self, mass: f32) -> GenericResult<()>;
}
impl<InnerApp: Application + Send + Sync> NBodyApplication for GraphicalApplication<InnerApp> {
    fn spawn_bodies(&mut self, scene: Scene) -> GenericResult<()> {
        let body_model = self.load_model("cube.obj")?;
        let mut random = rand::thread_rng();

        for _ in 0..scene.body_count {
            let position_distribution =
                Uniform::new(scene.initial_position_min, scene.initial_position_max);
            let RandomPosition(position): RandomPosition = random.sample(position_distribution);

            let mass_distribution = Uniform::new(scene.minimum_mass, scene.maximum_mass);
            let mass: Mass = random.sample(mass_distribution);

            let velocity_distribution =
                Uniform::new(scene.initial_velocity_min, scene.initial_velocity_max);
            let velocity: Velocity = random.sample(velocity_distribution);

            let acceleration_distribution = Uniform::new(
                scene.initial_acceleration_min,
                scene.initial_acceleration_max,
            );
            let acceleration: Acceleration = random.sample(acceleration_distribution);

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

    fn spawn_sun(&mut self, mass: f32) -> GenericResult<()> {
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
        self.add_component(light_source, Mass(mass))?;
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
