use crossbeam::channel::{unbounded, Sender};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use rain_simulation::scene::create_evenly_interspersed_clouds;
use rain_simulation::{
    gravity, ground_collision, movement, rain, vapor_accumulation, wind, wind_direction,
};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static SHUTDOWN_SENDER: Mutex<Option<Sender<()>>> = Mutex::new(None);

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

        let total_duration = Arc::new(Mutex::new(None));
        let total_duration_ref = Arc::clone(&total_duration);

        let benchmark_system = move || {
            let mut start_time_guard = start_time.lock().unwrap();
            let start_time = start_time_guard.get_or_insert(Instant::now());

            if current_tick.load(Ordering::SeqCst) == target_tick_count {
                let mut total_duration_guard = total_duration_ref.lock().unwrap();
                *total_duration_guard = Some(start_time.elapsed());

                let mut shutdown_guard = SHUTDOWN_SENDER.lock().unwrap();
                drop(shutdown_guard.take());
            } else {
                current_tick.fetch_add(1, Ordering::SeqCst);
            }
        };

        let mut app = BasicApplicationBuilder::default()
            .add_system(gravity)
            .add_system(wind)
            .add_system(wind_direction)
            .add_system(movement)
            .add_system(rain)
            .add_system(ground_collision)
            .add_system(vapor_accumulation)
            .add_system(benchmark_system)
            .build();

        let clouds_components = create_evenly_interspersed_clouds(cloud_count);
        for cloud_components in clouds_components {
            app.create_entity(cloud_components).unwrap();
        }

        let (shutdown_sender, shutdown_receiver) = unbounded();
        {
            let mut shutdown_guard = SHUTDOWN_SENDER.lock().unwrap();
            *shutdown_guard = Some(shutdown_sender);
        }
        app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)
            .unwrap();

        let total_duration_guard = total_duration.lock().unwrap();
        total_duration_guard.unwrap()
    }
}
