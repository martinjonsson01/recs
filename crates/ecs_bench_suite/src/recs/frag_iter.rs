use crossbeam::channel::Sender;
use ecs::systems::{IntoSystem, System, Write};
use ecs::{Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder, Schedule};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::sync::{Mutex, OnceLock};

macro_rules! create_entities {
    ($app:ident; $( $variants:ident ),*) => {
        $(
            #[derive(Debug)]
            struct $variants(f32);

            for _ in 0..20 {
                let entity = $app.create_entity().unwrap();
                $app.add_component(entity, $variants(0.0)).unwrap();
                $app.add_component(entity, Data(0.0)).unwrap();
            }
        )*
    };
}

#[derive(Debug)]
struct Data(f32);

static APPLICATION: OnceLock<BasicApplication> = OnceLock::new();
static SCHEDULE: Mutex<Option<PrecedenceGraph>> = Mutex::new(None);
static SYSTEMS: OnceLock<Vec<Box<dyn System>>> = OnceLock::new();
static WORKER_SHUTDOWN_SENDER: OnceLock<Sender<()>> = OnceLock::new();

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        SYSTEMS.get_or_init(|| {
            let ab_system: Box<dyn System> = Box::new(double_data.into_system());
            vec![ab_system]
        });

        APPLICATION.get_or_init(|| {
            let mut app = BasicApplicationBuilder::default().build();

            create_entities!(app; A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);

            let worker_shutdown_sender = WorkerPool::initialize_global();
            WORKER_SHUTDOWN_SENDER.set(worker_shutdown_sender).unwrap();

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

        WorkerPool::execute_tick(schedule, &app.world).unwrap();
    }
}

fn double_data(mut data: Write<Data>) {
    data.0 *= 2.0;
}
