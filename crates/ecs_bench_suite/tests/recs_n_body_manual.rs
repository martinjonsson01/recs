use crossbeam::channel::{unbounded, Sender};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use n_body::scenes::{all_heavy_random_cube_with_bodies, create_planet_entity};
use n_body::{acceleration, gravity, movement, BodySpawner, GenericResult};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SIMULATED_TICKS: usize = 10_000;
const BODY_COUNT: u32 = 100;

static SHUTDOWN_SENDER: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

#[test]
#[ignore] // Takes too long to run as an ordinary test. Only run manually.
fn recs_n_body_manual() -> GenericResult<()> {
    let mut app = BasicApplicationBuilder::default()
        .add_system(movement)
        .add_system(acceleration)
        .add_system(gravity)
        .add_system(benchmark_system)
        .build();

    all_heavy_random_cube_with_bodies(BODY_COUNT)
        .spawn_bodies(&mut app, create_planet_entity)
        .unwrap();

    let (shutdown_sender, shutdown_receiver) = unbounded();
    SHUTDOWN_SENDER.get_or_init(|| Mutex::new(Some(shutdown_sender)));
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}

static TICKS: AtomicUsize = AtomicUsize::new(0);
static START_TIME: OnceLock<Instant> = OnceLock::new();

fn benchmark_system() {
    let start_time = START_TIME.get_or_init(Instant::now);

    if TICKS.load(Ordering::SeqCst) == SIMULATED_TICKS {
        let bench_duration = start_time.elapsed();
        print_benchmark_results(bench_duration);

        let mut shutdown_guard = SHUTDOWN_SENDER.get().unwrap().lock().unwrap();
        drop(shutdown_guard.take());
    }
    TICKS.fetch_add(1, Ordering::SeqCst);
}

fn print_benchmark_results(bench_duration: Duration) {
    let elapsed_seconds = bench_duration.as_secs_f32();
    let average_tps = (SIMULATED_TICKS as f32) / bench_duration.as_secs_f32();
    println!("Finished benchmark (took {elapsed_seconds} s)");
    println!("average TPS {average_tps}");
}
