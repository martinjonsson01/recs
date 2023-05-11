use bevy::app::{AppExit, ScheduleRunnerSettings};
use bevy::prelude::*;
use crossbeam::sync::Parker;
use rain_simulation::scene::create_evenly_interspersed_clouds;
use rain_simulation::{Position, FIXED_TIME_STEP};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Component)]
pub struct Cloud {
    /// How much water vapor is absorbed per second.
    pub vapor_accumulation_rate: f32,
    /// In how large an area rain-drops are dropped.
    pub drop_emit_area: Surface,
}

pub struct Surface {
    pub width: f32,
    pub depth: f32,
}

impl Surface {
    fn deterministic_point_on_surface(&self, center: Vec3) -> Vec3 {
        center
    }
}

#[derive(Component)]
pub struct Mass(pub f32);

#[derive(Component)]
pub struct Velocity(Vec3);

impl Default for Velocity {
    fn default() -> Self {
        Velocity(Vec3::ZERO)
    }
}

#[derive(Clone)]
pub struct Benchmark {
    cloud_count: u32,
    current_tick: Arc<AtomicU64>,
    start_time: Arc<Mutex<Option<Instant>>>,
}

impl Benchmark {
    pub fn new(cloud_count: u32) -> Self {
        Self {
            cloud_count,
            current_tick: Arc::new(AtomicU64::new(0)),
            start_time: Arc::new(Mutex::new(None)),
        }
    }

    pub fn run(&mut self, target_tick_count: u64) -> Duration {
        let Self {
            cloud_count,
            current_tick,
            start_time,
        } = self.clone();

        let mut app = App::new();

        let total_duration = Arc::new(Mutex::new(None));
        let total_duration_ref = Arc::clone(&total_duration);

        let shutdown_parker = Parker::new();
        let shutdown_unparker = shutdown_parker.unparker().clone();

        let benchmark_system = move |mut exit: EventWriter<AppExit>| {
            let mut start_time_guard = start_time.lock().unwrap();
            let start_time = start_time_guard.get_or_insert(Instant::now());

            if current_tick.load(Ordering::SeqCst) == target_tick_count {
                let mut total_duration_guard = total_duration_ref.lock().unwrap();
                *total_duration_guard = Some(start_time.elapsed());

                shutdown_unparker.unpark();
                exit.send(AppExit);
            } else {
                current_tick.fetch_add(1, Ordering::SeqCst);
            }
        };

        app.insert_resource(ScheduleRunnerSettings::run_loop(Duration::ZERO))
            .add_plugins(MinimalPlugins)
            .add_system(gravity)
            .add_system(wind)
            .add_system(wind_direction)
            .add_system(movement)
            .add_system(rain)
            .add_system(ground_collision)
            .add_system(vapor_accumulation)
            .add_system(benchmark_system);

        let clouds_components = create_evenly_interspersed_clouds(cloud_count);
        for cloud_components in clouds_components {
            spawn_cloud(&mut app, cloud_components);
        }

        app.run();

        shutdown_parker.park();

        let total_duration_guard = total_duration.lock().unwrap();
        total_duration_guard.unwrap()
    }
}

fn spawn_cloud(
    app: &mut App,
    cloud_components: (rain_simulation::Cloud, Position, rain_simulation::Mass),
) {
    let cloud = Cloud {
        vapor_accumulation_rate: cloud_components.0.vapor_accumulation_rate,
        drop_emit_area: Surface {
            width: cloud_components.0.drop_emit_area.width,
            depth: cloud_components.0.drop_emit_area.depth,
        },
    };

    let point = cloud_components.1.point;
    let transform = Transform::from_xyz(point.x, point.y, point.z);

    let mass = Mass(cloud_components.2 .0);

    app.world.spawn((cloud, transform, mass));
}

const GRAVITY_ACCELERATION: f32 = 9.82;
const TERMINAL_VELOCITY: f32 = 9.0;

pub fn gravity(mut query: Query<&mut Velocity>) {
    query.par_iter_mut().for_each_mut(|mut velocity| {
        if f32::abs(velocity.0.y) >= TERMINAL_VELOCITY {
            return;
        }
        velocity.0 += -GRAVITY_ACCELERATION * FIXED_TIME_STEP * Vec3::Y;
    });
}

const WIND_ACCELERATION: f32 = 10.0;

static WIND_DIRECTION: Mutex<Vec3> = Mutex::new(Vec3::new(1.0, 0.0, 0.0));
const WIND_ROTATION_SPEED: f32 = 10.0;

pub fn wind_direction() {
    let mut wind_direction = WIND_DIRECTION.lock().expect("lock shouldn't be poisoned");
    let rotation_axis = Vec3::Y;
    let rotation = Quat::from_axis_angle(rotation_axis, WIND_ROTATION_SPEED * FIXED_TIME_STEP);
    *wind_direction = rotation.mul_vec3(*wind_direction);
}

pub fn wind(mut query: Query<&mut Velocity>) {
    query.par_iter_mut().for_each_mut(|mut velocity| {
        if velocity.0.length() >= TERMINAL_VELOCITY {
            return;
        }
        let wind_direction = WIND_DIRECTION.lock().expect("lock shouldn't be poisoned");
        velocity.0 += WIND_ACCELERATION * FIXED_TIME_STEP * *wind_direction;
    });
}

pub fn movement(mut query: Query<(&mut Transform, &Velocity)>) {
    query
        .par_iter_mut()
        .for_each_mut(|(mut transform, velocity)| {
            transform.translation += velocity.0 * FIXED_TIME_STEP;
        });
}

const MASS_PER_RAINDROP: f32 = 0.1;

pub fn rain(mut query: Query<(&Cloud, &Transform, &mut Mass)>, mut commands: Commands) {
    // Can't do this in parallel since it uses command buffer.
    for (cloud, transform, mut mass) in query.iter_mut() {
        while mass.0 > MASS_PER_RAINDROP {
            let drop_position = cloud
                .drop_emit_area
                .deterministic_point_on_surface(transform.translation);
            commands.spawn((
                Velocity::default(),
                Mass(MASS_PER_RAINDROP),
                Transform::from_translation(drop_position),
            ));

            mass.0 -= MASS_PER_RAINDROP;
        }
    }
}

const GROUND_HEIGHT: f32 = 0.0;

pub fn ground_collision(query: Query<(Entity, &Transform)>, mut commands: Commands) {
    // Can't do this in parallel since it uses command buffer.
    for (entity, transform) in query.iter() {
        if transform.translation.y <= GROUND_HEIGHT {
            commands.entity(entity).despawn();
        }
    }
}

pub fn vapor_accumulation(mut query: Query<(&Cloud, &mut Mass)>) {
    query.par_iter_mut().for_each_mut(|(cloud, mut mass)| {
        mass.0 += cloud.vapor_accumulation_rate * FIXED_TIME_STEP;
    });
}
