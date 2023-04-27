use crossbeam::channel::{unbounded, Sender};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder, BasicApplicationError};
use n_body::scenes::{all_heavy_random_cube_with_bodies, create_planet_entity};
use n_body::{acceleration, gravity, movement, BodySpawner, GenericResult};
use num_integer::Roots;
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::io::{stdout, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

static SHUTDOWN_SENDER: Mutex<Option<Sender<()>>> = Mutex::new(None);

#[derive(Clone)]
struct Benchmark {
    body_count: u32,
    ticks_per_sample: u32,
    target_tick_count: u32,
    current_tick: Arc<AtomicU32>,
    /// When the last sample was taken.
    previous_sample_time: Arc<Mutex<Option<Instant>>>,
    /// How much time has passed between one sample and the next.
    sample_intervals: Arc<Mutex<Vec<Duration>>>,
}

impl Benchmark {
    fn new(body_count: u32, target_tick_count: u32) -> Self {
        // How many times during the benchmark execution that samples will be taken.
        const SAMPLE_COUNT: u32 = 10;
        let ticks_per_sample = target_tick_count / SAMPLE_COUNT;
        Self {
            body_count,
            ticks_per_sample,
            target_tick_count,
            current_tick: Arc::new(AtomicU32::new(0)),
            previous_sample_time: Arc::new(Mutex::new(None)),
            sample_intervals: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn run(&mut self) -> Result<(), BasicApplicationError> {
        let Self {
            body_count,
            ticks_per_sample,
            target_tick_count,
            current_tick,
            previous_sample_time,
            sample_intervals,
        } = self.clone();

        let benchmark_system = move || {
            let current_tick_value = current_tick.load(Ordering::SeqCst);

            // Periodically take new samples of how long time has passed since last sample.
            if current_tick_value % ticks_per_sample == 0 {
                let mut maybe_previous_sample_time = previous_sample_time.lock().unwrap();

                if let Some(previous_sample_time) = maybe_previous_sample_time.as_ref() {
                    let sample_duration = previous_sample_time.elapsed();

                    let mut sample_intervals = sample_intervals.lock().unwrap();
                    sample_intervals.push(sample_duration);
                    print!(".");
                    stdout().flush().unwrap();
                } else {
                    _ = maybe_previous_sample_time.insert(Instant::now());
                }
            }

            if current_tick_value == target_tick_count {
                let mut shutdown_guard = SHUTDOWN_SENDER.lock().unwrap();
                drop(shutdown_guard.take());
            } else {
                current_tick.fetch_add(1, Ordering::SeqCst);
            }
        };

        let mut app = BasicApplicationBuilder::default()
            .add_system(movement)
            .add_system(acceleration)
            .add_system(gravity)
            .add_system(benchmark_system)
            .build();

        all_heavy_random_cube_with_bodies(body_count)
            .spawn_bodies(&mut app, create_planet_entity)
            .unwrap();

        let (shutdown_sender, shutdown_receiver) = unbounded();
        {
            let mut shutdown_guard = SHUTDOWN_SENDER.lock().unwrap();
            *shutdown_guard = Some(shutdown_sender);
        }
        print!("benchmarking {body_count} bodies for {target_tick_count} ticks");
        app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;
        println!(" done!");

        Ok(())
    }

    fn print_benchmark_results(&self) {
        let sample_intervals = self.sample_intervals.lock().unwrap();
        let sample_intervals_count = sample_intervals.len() as u32;

        let mean_tick_duration =
            self.calculate_mean_tick_duration(&sample_intervals, sample_intervals_count);

        let standard_deviation_nanos = calculate_standard_deviation(
            sample_intervals,
            sample_intervals_count,
            mean_tick_duration,
        );

        let body_count = self.body_count;
        let ticks = self.current_tick.load(Ordering::SeqCst);
        println!("Benchmark result for {body_count} bodies over {ticks} ticks:");

        let mean_ticks_per_second =
            (self.ticks_per_sample as f64) / mean_tick_duration.as_secs_f64();
        let mean_tick_duration_ms = mean_tick_duration.as_secs_f64() / 10e3;
        print!(
            "Avg. TPS: {mean_ticks_per_second:.2} Avg. tick time (ms): {mean_tick_duration_ms:.6} "
        );

        let standard_deviation_ms = (standard_deviation_nanos as f64) / 10e6;
        println!("Standard deviation (ms): {standard_deviation_ms:.2}")
    }

    fn calculate_mean_tick_duration(
        &self,
        sample_intervals: &MutexGuard<Vec<Duration>>,
        sample_intervals_count: u32,
    ) -> Duration {
        let mean_sample_interval_duration =
            sample_intervals.iter().sum::<Duration>() / sample_intervals_count;
        mean_sample_interval_duration / self.ticks_per_sample
    }
}

fn calculate_standard_deviation(
    sample_intervals: MutexGuard<Vec<Duration>>,
    sample_intervals_count: u32,
    mean_tick_duration: Duration,
) -> u128 {
    let mut variance_nanos = sample_intervals
        .iter()
        .map(|&sample_duration| {
            let difference_to_mean_nanos = if mean_tick_duration > sample_duration {
                mean_tick_duration.as_nanos() - sample_duration.as_nanos()
            } else {
                sample_duration.as_nanos() - mean_tick_duration.as_nanos()
            };
            difference_to_mean_nanos * difference_to_mean_nanos
        })
        .sum::<u128>();
    variance_nanos /= (sample_intervals_count - 1) as u128;

    variance_nanos.sqrt()
}

#[test]
#[ignore] // Takes too long to run as an ordinary test. Only run manually.
fn recs_n_body_manual() -> GenericResult<()> {
    let benchmarks = [
        Benchmark::new(100, 100_000),
        Benchmark::new(500, 4_000),
        Benchmark::new(1_000, 1_000),
        Benchmark::new(5_000, 400),
        Benchmark::new(10_000, 100),
        Benchmark::new(100_000, 10),
    ];

    for mut bench in benchmarks {
        println!();
        bench.run()?;
        bench.print_benchmark_results();
    }

    Ok(())
}
