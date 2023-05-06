use crate::{Acceleration, BodySpawner, GenericResult, Mass, RandomPosition, Velocity};
use cgmath::{Array, InnerSpace, Vector3};
use ecs::{Application, IntoTickable};
use gfx_plugin::rendering::{Model, PointLight, Position};
use gfx_plugin::GraphicalApplication;
use rand::distributions::Uniform;
use rand::Rng;
use std::sync::Mutex;

const EVERYTHING_HEAVY: Scene = Scene {
    body_count: 1_000,
    initial_position_min: -100.0,
    initial_position_max: 100.0,
    position_offset: Vector3::new(0.0, 0.0, 0.0),
    minimum_mass: 5.9000e14,
    maximum_mass: 5.9724e14,
    initial_velocity_min: -10_000.0,
    initial_velocity_max: 10_000.0,
    initial_acceleration_min: -1.0,
    initial_acceleration_max: 1.0,
};

const EVERYTHING_LIGHT: Scene = Scene {
    minimum_mass: 100.0,
    maximum_mass: 1_000.0,
    ..EVERYTHING_HEAVY
};

const ONE_HEAVY_BODY: Scene = Scene {
    body_count: 1,
    initial_position_min: -10.0,
    initial_position_max: 10.0,
    position_offset: Vector3::new(0.0, 0.0, 0.0),
    minimum_mass: 1.900e20,
    maximum_mass: 1.989e20,
    initial_velocity_min: 0.0,
    initial_velocity_max: 1e-30,
    initial_acceleration_min: 0.0,
    initial_acceleration_max: 1e-30,
};

#[derive(Copy, Clone)]
pub struct RandomCubeSpawner(Scene);

pub const ALL_LIGHT_RANDOM_CUBE: RandomCubeSpawner = RandomCubeSpawner(EVERYTHING_LIGHT);
pub const ALL_HEAVY_RANDOM_CUBE: RandomCubeSpawner = RandomCubeSpawner(EVERYTHING_HEAVY);
pub const SINGLE_HEAVY_BODY_AT_ORIGIN: RandomCubeSpawner = RandomCubeSpawner(ONE_HEAVY_BODY);

pub fn all_heavy_random_cube_with_bodies(body_count: u32) -> RandomCubeSpawner {
    let scene = Scene {
        body_count,
        ..EVERYTHING_HEAVY
    };
    RandomCubeSpawner(scene)
}

impl<App> BodySpawner<App> for RandomCubeSpawner {
    fn spawn_bodies<CreateEntityFn>(
        &self,
        app: &mut App,
        create_entity: CreateEntityFn,
    ) -> GenericResult<()>
    where
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
}

impl RandomCubeSpawner {
    pub fn with_offset(mut self, offset: Vector3<f32>) -> Self {
        self.0.position_offset = offset;
        self
    }
    pub fn with_velocity_range(mut self, velocity: f32) -> Self {
        self.0.initial_velocity_min = -velocity;
        self.0.initial_velocity_max = velocity;
        self
    }
}

pub fn create_planet_entity<App: Application>(
    app: &mut App,
    position: Position,
    mass: Mass,
    velocity: Velocity,
    acceleration: Acceleration,
) -> GenericResult<()> {
    app.create_entity((position, mass, velocity, acceleration))?;
    Ok(())
}

static MOON_MODEL: Mutex<Option<Model>> = Mutex::new(None);

pub fn create_rendered_planet_entity<InnerApp: Application + Send + Sync + IntoTickable>(
    app: &mut GraphicalApplication<InnerApp>,
    position: Position,
    mass: Mass,
    velocity: Velocity,
    acceleration: Acceleration,
) -> GenericResult<()> {
    let body_model = *MOON_MODEL
        .lock()
        .expect("lock shouldn't be poisoned")
        .get_or_insert_with(|| app.load_model("moon.obj").expect("moon.obj file exists"));

    let entity = app
        .rendered_entity_builder(body_model)?
        .with_position(position)
        .build()?;

    app.add_component(entity, mass)?;
    app.add_component(entity, velocity)?;
    app.add_component(entity, acceleration)?;

    Ok(())
}

pub fn create_rendered_sun_entity<InnerApp: Application + Send + Sync + IntoTickable>(
    app: &mut GraphicalApplication<InnerApp>,
    position: Position,
    mass: Mass,
    velocity: Velocity,
    acceleration: Acceleration,
) -> GenericResult<()> {
    app.create_entity((
        PointLight {
            color: [
                rand::thread_rng().gen_range(0.0..1.0),
                rand::thread_rng().gen_range(0.0..1.0),
                rand::thread_rng().gen_range(0.0..1.0),
            ]
            .into(),
        },
        position,
        mass,
        velocity,
        acceleration,
    ))?;

    Ok(())
}

#[derive(Copy, Clone)]
pub struct ClusterSpawner {
    cluster_count: u32,
    cluster_size: f32,
    cluster_distance: f32,
    scene: Scene,
}

pub const SMALL_HEAVY_CLUSTERS: ClusterSpawner = ClusterSpawner {
    cluster_count: 4,
    cluster_size: 10.0,
    cluster_distance: 100.0,
    scene: EVERYTHING_HEAVY,
};

pub const SMALL_LIGHT_CLUSTERS: ClusterSpawner = ClusterSpawner {
    scene: EVERYTHING_LIGHT,
    ..SMALL_HEAVY_CLUSTERS
};

pub const HUGE_HEAVY_CLUSTERS: ClusterSpawner = ClusterSpawner {
    cluster_count: 4,
    cluster_size: 100.0,
    cluster_distance: 1000.0,
    scene: Scene {
        minimum_mass: 100_000.0,
        maximum_mass: 1_000_000.0,
        ..EVERYTHING_HEAVY
    },
};

pub const HUGE_LIGHT_CLUSTERS: ClusterSpawner = ClusterSpawner {
    scene: EVERYTHING_LIGHT,
    ..HUGE_HEAVY_CLUSTERS
};

impl<App> BodySpawner<App> for ClusterSpawner {
    fn spawn_bodies<CreateEntityFn>(
        &self,
        app: &mut App,
        create_entity: CreateEntityFn,
    ) -> GenericResult<()>
    where
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
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Scene {
    body_count: u32,
    initial_position_min: f32,
    initial_position_max: f32,
    position_offset: Vector3<f32>,
    minimum_mass: f32,
    maximum_mass: f32,
    initial_velocity_min: f32,
    initial_velocity_max: f32,
    initial_acceleration_min: f32,
    initial_acceleration_max: f32,
}
