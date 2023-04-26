pub mod scenes;

use bevy_ecs::prelude::Component;
use cgmath::{InnerSpace, Point3, Vector3, Zero};
use color_eyre::Report;
use ecs::systems::{Read, Write};
use gfx_plugin::rendering;
use rand::distributions::Uniform;
use rand::prelude::Distribution;
use rand::Rng;
use std::ops::{Deref, DerefMut};
use tracing::instrument;

// todo(#90): change to use dynamic delta time.
// todo(#90) currently assuming a hardcoded tick rate.
const FIXED_TIME_STEP: f32 = 1.0 / 20000.0;

pub trait BodySpawner<App> {
    fn spawn_bodies<CreateEntityFn>(
        &self,
        app: &mut App,
        create_entity: CreateEntityFn,
    ) -> GenericResult<()>
    where
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
pub struct Mass(f32);

impl Distribution<Mass> for Uniform<f32> {
    fn sample<R: Rng + ?Sized>(&self, random: &mut R) -> Mass {
        Mass(self.sample(random))
    }
}

/// How fast a body is moving, in meters/second.
#[derive(Debug, Component)]
pub struct Velocity(Vector3<f32>);

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
#[derive(Debug, Component)]
pub struct Acceleration(Vector3<f32>);

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

pub type GenericResult<T> = Result<T, Report>;

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
pub fn recs_movement(mut position: Write<Position>, velocity: Read<Velocity>) {
    move_position(&mut position, &velocity);
}

pub fn bevy_movement(mut query: bevy_ecs::prelude::Query<(&mut Position, &Velocity)>) {
    query
        .par_iter_mut()
        .for_each_mut(|(mut position, velocity)| {
            move_position(&mut position, velocity);
        });
}

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
pub fn recs_acceleration(mut velocity: Write<Velocity>, acceleration: Read<Acceleration>) {
    accelerate_velocity(&mut velocity, &acceleration);
}

pub fn bevy_acceleration(mut query: bevy_ecs::prelude::Query<(&mut Velocity, &Acceleration)>) {
    query
        .par_iter_mut()
        .for_each_mut(|(mut velocity, acceleration)| {
            accelerate_velocity(&mut velocity, acceleration);
        });
}

#[instrument(skip_all)]
#[cfg_attr(feature = "profile", inline(never))]
pub fn recs_gravity(
    position: Read<Position>,
    mut acceleration: Write<Acceleration>,
    bodies_query: ecs::systems::Query<(Read<Position>, Read<Mass>)>,
) {
    acceleration_due_to_gravity(&position, &mut acceleration, bodies_query.into_iter());
}

pub fn bevy_gravity(
    mut self_query: bevy_ecs::prelude::Query<(&Position, &mut Acceleration)>,
    body_query: bevy_ecs::prelude::Query<(&Position, &Mass)>,
) {
    self_query
        .par_iter_mut()
        .for_each_mut(|(position, mut acceleration)| {
            acceleration_due_to_gravity(position, &mut acceleration, body_query.iter());
        });
}

/// A common implementation of the movement system.
pub fn move_position(position: &mut Position, velocity: &Velocity) {
    let Velocity(velocity) = velocity;
    let Position(ref mut position) = position;

    position.point += velocity * FIXED_TIME_STEP;
}

/// A common implementation of the acceleration system.
pub fn accelerate_velocity(velocity: &mut Velocity, acceleration: &Acceleration) {
    let Velocity(ref mut velocity) = velocity;
    let Acceleration(acceleration) = acceleration;

    *velocity += acceleration * FIXED_TIME_STEP;
}

/// A common implementation of the gravity system.
pub fn acceleration_due_to_gravity(
    position: &Position,
    acceleration: &mut Acceleration,
    bodies_query: impl Iterator<Item = (impl Deref<Target = Position>, impl Deref<Target = Mass>)>,
) {
    let Position(rendering::Position { point: position }) = position;
    let Acceleration(ref mut acceleration) = acceleration;

    let acceleration_towards_body = |(body_position, body_mass): (Point3<f32>, f32)| {
        let to_body: Vector3<f32> = (body_position - position)
            .cast()
            .expect("f32 -> f32 cast always works");
        let distance_squared = to_body.magnitude2();

        if distance_squared <= f32::EPSILON {
            return Vector3::zero();
        }

        // Newton's law of universal gravitation.
        const GRAVITATIONAL_CONSTANT: f32 = 6.67e-11;
        let acceleration = GRAVITATIONAL_CONSTANT * body_mass / distance_squared;

        to_body.normalize_to(acceleration)
    };

    let total_acceleration: Vector3<f32> = bodies_query
        .into_iter()
        .map(|(position, mass)| (position.point, mass.0))
        .map(acceleration_towards_body)
        .sum();
    *acceleration = total_acceleration;
}
