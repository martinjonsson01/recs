use cgmath::*;
use ecs::systems::{IntoSystem, Read, System, Write};
use ecs::{
    Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder, Executor, Schedule,
    Sequential,
};
use scheduler::schedule::PrecedenceGraph;
use std::sync::{Mutex, OnceLock};

#[derive(Copy, Clone, Debug)]
struct Transform(Matrix4<f32>);

#[derive(Copy, Clone, Debug)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Velocity(Vector3<f32>);

static APPLICATION: OnceLock<BasicApplication> = OnceLock::new();
static SCHEDULE: Mutex<Option<PrecedenceGraph>> = Mutex::new(None);
static SYSTEMS: OnceLock<Vec<Box<dyn System>>> = OnceLock::new();

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        SYSTEMS.get_or_init(|| {
            let movement_system: Box<dyn System> = Box::new(movement_system.into_system());
            vec![movement_system]
        });

        APPLICATION.get_or_init(|| {
            let mut app = BasicApplicationBuilder::default().build();

            for _ in 0..10_000 {
                let entity = app.create_entity().unwrap();
                app.add_component(entity, Matrix4::from_scale(1.0)).unwrap();
                app.add_component(entity, Position(Vector3::unit_x()))
                    .unwrap();
                app.add_component(entity, Rotation(Vector3::unit_x()))
                    .unwrap();
                app.add_component(entity, Velocity(Vector3::unit_x()))
                    .unwrap();
            }

            let systems = SYSTEMS.get().unwrap();
            let schedule = PrecedenceGraph::generate(systems).unwrap();
            let mut schedule_guard = SCHEDULE.lock().unwrap();
            *schedule_guard = Some(schedule);

            app
        });

        Self
    }

    pub fn run(&mut self) {
        let app = APPLICATION.get().unwrap();
        let mut schedule_guard = SCHEDULE.lock().unwrap();
        let schedule = schedule_guard.as_mut().unwrap();

        Sequential.execute_once(schedule, &app.world).unwrap();
    }
}

fn movement_system(mut position: Write<Position>, velocity: Read<Velocity>) {
    position.0 += velocity.0;
}
