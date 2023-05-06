use cgmath::{Point3, Vector3, Zero};
use ecs::systems::command_buffers::Commands;
use ecs::systems::{Read, Write};
use ecs::Entity;
use gfx_plugin::rendering::{Model, Position, Rotation, Scale};
use rand::{thread_rng, Rng};
use std::sync::OnceLock;

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

const WIND_ACCELERATION: f32 = 3.0;

pub fn wind(mut velocity: Write<Velocity>) {
    if f32::abs(velocity.0.x) >= TERMINAL_VELOCITY {
        return;
    }
    velocity.0 += WIND_ACCELERATION * FIXED_TIME_STEP * Vector3::unit_x();
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
            Rotation::default(),
            Scale::default(),
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
