pub mod scenes;

use bevy_ecs::prelude::Component;
use cgmath::{InnerSpace, MetricSpace, Point3, Vector3, Zero};
use color_eyre::Report;
use ecs::systems::{Query, Read, Write};
use ecs::Application;
use gfx_plugin::rendering;
use rand::distributions::Uniform;
use rand::prelude::Distribution;
use rand::Rng;
use std::ops::{Deref, DerefMut};
use tracing::instrument;

// todo(#90): change to use dynamic delta time.
// todo(#90) currently assuming a hardcoded tick rate.
const FIXED_TIME_STEP: f32 = 1.0 / 20000.0;

pub trait BodySpawner {
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
#[derive(Debug, Component)]
pub struct Position(rendering::Position);

impl Deref for Position {
    type Target = rendering::Position;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Position {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Distribution<Position> for Uniform<f32> {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Position {
        Position(rendering::Position {
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
#[derive(Debug, Component)]
pub struct Mass(f64);

impl Distribution<Mass> for Uniform<f64> {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Mass {
        Mass(self.sample(random))
    }
}

/// How fast a body is moving, in meters/second.
#[derive(Debug, Component)]
pub struct Velocity(Vector3<f64>);

impl Distribution<Velocity> for Uniform<f64> {
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
#[derive(Debug, Component)]
pub struct Acceleration(Vector3<f64>);

impl Distribution<Acceleration> for Uniform<f64> {
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

pub type GenericResult<T> = Result<T, Report>;

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
pub fn movement(mut position: Write<Position>, velocity: Read<Velocity>) {
    move_position(&velocity, &mut position);
}

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
pub fn acceleration(mut velocity: Write<Velocity>, acceleration: Read<Acceleration>) {
    accelerate_velocity(&mut velocity, &acceleration);
}

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
pub fn gravity(
    position: Read<Position>,
    mut acceleration: Write<Acceleration>,
    bodies_query: Query<(Read<Position>, Read<Mass>)>,
) {
    acceleration_due_to_gravity(&position, &mut acceleration, bodies_query.into_iter());
}

/// A common implementation of the movement system.
pub fn move_position(velocity: &Velocity, position: &mut Position) {
    let Velocity(velocity) = velocity;
    let Position(ref mut position) = position;

    position.point += velocity.map(|coord| coord as f32) * FIXED_TIME_STEP;
}

/// A common implementation of the acceleration system.
pub fn accelerate_velocity(velocity: &mut Velocity, acceleration: &Acceleration) {
    let Velocity(ref mut velocity) = velocity;
    let Acceleration(acceleration) = acceleration;

    *velocity += acceleration * (FIXED_TIME_STEP as f64);
}

/// A common implementation of the gravity system.
pub fn acceleration_due_to_gravity<'a>(
    position: &Position,
    acceleration: &mut Acceleration,
    bodies_query: impl Iterator<Item = (Read<'a, Position>, Read<'a, Mass>)>,
) {
    let Position(rendering::Position { point: position }) = position;
    let Acceleration(ref mut acceleration) = acceleration;

    let acceleration_towards_body = |(body_position, body_mass): (Point3<f32>, f64)| {
        let to_body: Vector3<f64> = (body_position - position)
            .cast()
            .expect("f32 -> f64 cast always works");
        let distance_squared = to_body.distance2(Vector3::zero());

        if distance_squared <= f64::EPSILON {
            return Vector3::zero();
        }

        // Newton's law of universal gravitation.
        const GRAVITATIONAL_CONSTANT: f64 = 6.67e-11;
        let acceleration = GRAVITATIONAL_CONSTANT * body_mass / distance_squared;

        to_body.normalize() * acceleration
    };

    let total_acceleration: Vector3<f64> = bodies_query
        .into_iter()
        .map(|(position, mass)| (position.point, mass.0))
        .map(acceleration_towards_body)
        .sum();
    *acceleration = total_acceleration;
}