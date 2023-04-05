use crate::{Acceleration, BodySpawner, GenericResult, Mass, RandomPosition, Velocity};
use cgmath::{Array, InnerSpace, Vector3};
use ecs::Application;
use gfx_plugin::rendering::Position;
use rand::distributions::Uniform;
use rand::Rng;

const EVERYTHING_HEAVY: Scene = Scene {
    body_count: 1_000,
    initial_position_min: -100.0,
    initial_position_max: 100.0,
    position_offset: Vector3::new(0.0, 0.0, 0.0),
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

#[derive(Copy, Clone)]
pub struct RandomCubeSpawner(Scene);

pub const UNEVEN_WEIGHTS_RANDOM_CUBE: RandomCubeSpawner = RandomCubeSpawner(LIGHT_BODIES_HEAVY_SUN);
pub const ALL_HEAVY_RANDOM_CUBE: RandomCubeSpawner = RandomCubeSpawner(EVERYTHING_HEAVY);

impl BodySpawner for RandomCubeSpawner {
    fn spawn_bodies<App, CreateEntityFn>(
        &self,
        app: &mut App,
        create_entity: CreateEntityFn,
    ) -> GenericResult<()>
    where
        App: Application,
        CreateEntityFn: Fn(&mut App, Position, Mass, Velocity, Acceleration) -> GenericResult<()>,
    {
        let RandomCubeSpawner(scene) = self;

        let mut random = rand::thread_rng();

        for _ in 0..scene.body_count {
            let position_distribution =
                Uniform::new(scene.initial_position_min, scene.initial_position_max);
            let RandomPosition(mut position): RandomPosition = random.sample(position_distribution);
            position.point += scene.position_offset;

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

            create_entity(app, position, mass, velocity, acceleration)?;
        }

        Ok(())
    }

    fn sun_mass(&self) -> f32 {
        self.0.sun_mass
    }
}

#[derive(Copy, Clone)]
pub struct ClusterSpawner {
    cluster_count: u32,
    cluster_size: f32,
    cluster_distance: f32,
    scene: Scene,
}

pub const SMALL_CLUSTERS: ClusterSpawner = ClusterSpawner {
    cluster_count: 4,
    cluster_size: 10.0,
    cluster_distance: 100.0,
    scene: EVERYTHING_HEAVY,
};

pub const HUGE_CLUSTERS: ClusterSpawner = ClusterSpawner {
    cluster_count: 4,
    cluster_size: 100.0,
    cluster_distance: 1000.0,
    scene: Scene {
        minimum_mass: 100_000.0,
        maximum_mass: 1_000_000.0,
        sun_mass: 1_000_000.0,
        ..EVERYTHING_HEAVY
    },
};

impl BodySpawner for ClusterSpawner {
    fn spawn_bodies<App, CreateEntityFn>(
        &self,
        app: &mut App,
        create_entity: CreateEntityFn,
    ) -> GenericResult<()>
    where
        App: Application,
        CreateEntityFn: Fn(&mut App, Position, Mass, Velocity, Acceleration) -> GenericResult<()>,
    {
        let ClusterSpawner {
            cluster_count,
            cluster_size,
            cluster_distance,
            scene,
        } = *self;

        let mut random = rand::thread_rng();

        let previous_cluster_position = Vector3::from_value(0.0);
        for _ in 0..cluster_count {
            let random_direction: Vector3<f32> = [
                random.gen_range(-1.0..1.0),
                random.gen_range(-1.0..1.0),
                random.gen_range(-1.0..1.0),
            ]
            .into();
            let random_direction = random_direction.normalize();

            let new_cluster_position =
                previous_cluster_position + random_direction * cluster_distance;

            let cluster_scene = Scene {
                body_count: scene.body_count / cluster_count,
                position_offset: new_cluster_position,
                initial_position_min: -cluster_size,
                initial_position_max: cluster_size,
                ..scene
            };
            let cluster_spawner = RandomCubeSpawner(cluster_scene);
            cluster_spawner.spawn_bodies(app, &create_entity)?;
        }

        Ok(())
    }

    fn sun_mass(&self) -> f32 {
        self.scene.sun_mass
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Scene {
    body_count: u32,
    initial_position_min: f32,
    initial_position_max: f32,
    position_offset: Vector3<f32>,
    minimum_mass: f32,
    maximum_mass: f32,
    sun_mass: f32,
    initial_velocity_min: f32,
    initial_velocity_max: f32,
    initial_acceleration_min: f32,
    initial_acceleration_max: f32,
}
