use cgmath::{Deg, InnerSpace, Point3, Quaternion, Rotation, Rotation3, Vector3, Zero};
use ecs::systems::command_buffers::Commands;
use ecs::systems::{Read, Write};
use ecs::Entity;
use gfx_plugin::rendering::{Model, Position, Scale};
use rand::{thread_rng, Rng};
use std::sync::{Mutex, OnceLock};

pub mod scene;

// todo(#90): change to use dynamic delta time.
// todo(#90): currently assuming a hardcoded tick rate.
pub const FIXED_TIME_STEP: f32 = 1.0 / 20.0;

#[derive(Debug)]
pub struct Cloud {
    /// How much water vapor is absorbed per second.
    pub vapor_accumulation_rate: f32,
    /// In how large an area rain-drops are dropped.
    pub drop_emit_area: Surface,
}

#[derive(Debug)]
pub struct Surface {
    pub width: f32,
    pub depth: f32,
}

impl Surface {
    fn random_point_on_surface(&self, center: Point3<f32>) -> Point3<f32> {
        let min_x = center.x - self.width / 2.0;
        let max_x = center.x + self.width / 2.0;
        let min_z = center.z - self.depth / 2.0;
        let max_z = center.z + self.depth / 2.0;
        [
            thread_rng().gen_range(min_x..=max_x),
            center.y,
            thread_rng().gen_range(min_z..=max_z),
        ]
        .into()
    }

    fn deterministic_point_on_surface(&self, center: Point3<f32>) -> Point3<f32> {
        center
    }
}

#[derive(Debug)]
pub struct RainDrop;

#[derive(Debug)]
pub struct Mass(pub f32);

#[derive(Debug)]
pub struct Velocity(Vector3<f32>);

impl Default for Velocity {
    fn default() -> Self {
        Velocity(Vector3::zero())
    }
}

const GRAVITY_ACCELERATION: f32 = 9.82;
const TERMINAL_VELOCITY: f32 = 9.0;

pub fn gravity(mut velocity: Write<Velocity>) {
    if f32::abs(velocity.0.y) >= TERMINAL_VELOCITY {
        return;
    }
    velocity.0 += -GRAVITY_ACCELERATION * FIXED_TIME_STEP * Vector3::unit_y();
}

const WIND_ACCELERATION: f32 = 10.0;

// Since we don't have resources yet, use a static variable...
static WIND_DIRECTION: Mutex<Vector3<f32>> = Mutex::new(Vector3::new(1.0, 0.0, 0.0));
const WIND_ROTATION_SPEED: Deg<f32> = Deg(10.0);

pub fn wind_direction() {
    let mut wind_direction = WIND_DIRECTION.lock().expect("lock shouldn't be poisoned");
    let rotation_axis = Vector3::unit_y();
    let rotation =
        Quaternion::from_axis_angle(rotation_axis, WIND_ROTATION_SPEED * FIXED_TIME_STEP);
    *wind_direction = rotation.rotate_vector(*wind_direction);
}

pub fn wind(mut velocity: Write<Velocity>) {
    if velocity.0.magnitude() >= TERMINAL_VELOCITY {
        return;
    }
    let wind_direction = WIND_DIRECTION.lock().expect("lock shouldn't be poisoned");
    velocity.0 += WIND_ACCELERATION * FIXED_TIME_STEP * *wind_direction;
}

pub fn movement(mut position: Write<Position>, velocity: Read<Velocity>) {
    position.point += velocity.0 * FIXED_TIME_STEP;
}

const MASS_PER_RAINDROP: f32 = 0.1;
pub static RAINDROP_MODEL: OnceLock<Model> = OnceLock::new();

pub fn rain_visual(
    cloud: Read<Cloud>,
    position: Read<Position>,
    mut mass: Write<Mass>,
    commands: Commands,
) {
    let model = RAINDROP_MODEL.get().expect("model should be initialized");

    while mass.0 > MASS_PER_RAINDROP {
        let drop_position = cloud.drop_emit_area.random_point_on_surface(position.point);
        commands.create((
            RainDrop,
            Velocity::default(),
            Mass(MASS_PER_RAINDROP),
            *model,
            Position {
                point: drop_position,
            },
            gfx_plugin::rendering::Rotation::default(),
            Scale {
                vector: Vector3::new(0.1, 0.1, 0.1),
            },
        ));

        mass.0 -= MASS_PER_RAINDROP;
    }
}

pub fn rain(
    cloud: Read<Cloud>,
    position: Read<Position>,
    mut mass: Write<Mass>,
    commands: Commands,
) {
    while mass.0 > MASS_PER_RAINDROP {
        let drop_position = cloud
            .drop_emit_area
            .deterministic_point_on_surface(position.point);
        commands.create((
            RainDrop,
            Velocity::default(),
            Mass(MASS_PER_RAINDROP),
            Position {
                point: drop_position,
            },
        ));

        mass.0 -= MASS_PER_RAINDROP;
    }
}

const GROUND_HEIGHT: f32 = 0.0;

pub fn ground_collision(entity: Entity, position: Read<Position>, commands: Commands) {
    if position.point.y <= GROUND_HEIGHT {
        commands.remove(entity);
    }
}

pub fn vapor_accumulation(cloud: Read<Cloud>, mut mass: Write<Mass>) {
    mass.0 += cloud.vapor_accumulation_rate * FIXED_TIME_STEP;
}
