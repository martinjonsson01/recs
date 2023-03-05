use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use crossbeam::channel::{unbounded, Receiver};
use ecs::pool::ThreadPool;
use ecs::scheduler_rayon::RayonChaos;
use ecs::scheduling::{Schedule, ScheduleExecutor, Unordered};
use ecs::Application;
use std::thread;
use std::time::Duration;

fn create_application(number_of_systems: i32) -> (Application, Receiver<()>) {
    let mut app = Application::default();

    let (completion_sender, completion_listener) = unbounded();

    for _ in 0..number_of_systems {
        let completion_sender = completion_sender.clone();
        let independent_system = move || {
            thread::sleep(Duration::from_millis(10));
            completion_sender.send(()).unwrap();
        };
        app = app.add_system(independent_system);
    }

    (app, completion_listener)
}

fn run_application_until_finished<'a, E, S>(
    app: &'a mut Application,
    completion_listener: Receiver<()>,
    listen_for_completions: i32,
) where
    E: ScheduleExecutor<'a> + Default,
    S: Schedule<'a>,
{
    let (shutdown_sender, shutdown_receiver) = unbounded();

    let _completion_thread = thread::spawn(move || {
        for _ in 0..listen_for_completions {
            completion_listener.recv().unwrap();
        }
        drop(shutdown_sender);
    });

    app.run::<E, S>(shutdown_receiver);
}

fn compare_executors(c: &mut Criterion) {
    let mut group = c.benchmark_group("independent");

    for number_of_systems in (0..10).map(|n| 2_i32.pow(n)) {
        group.bench_with_input(
            BenchmarkId::new("RayonChaos", number_of_systems),
            &number_of_systems,
            |b, &number_of_systems| {
                b.iter(|| {
                    let (mut app, completion_listener) = create_application(number_of_systems);
                    run_application_until_finished::<RayonChaos, Unordered>(
                        &mut app,
                        #[allow(clippy::redundant_clone)] // To keep channel alive long enough.
                        completion_listener.clone(),
                        number_of_systems,
                    );
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("ThreadPool", number_of_systems),
            &number_of_systems,
            |b, &number_of_systems| {
                b.iter(|| {
                    let (mut app, completion_listener) = create_application(number_of_systems);
                    run_application_until_finished::<ThreadPool, Unordered>(
                        &mut app,
                        #[allow(clippy::redundant_clone)] // To keep channel alive long enough.
                        completion_listener.clone(),
                        number_of_systems,
                    );
                });
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = compare_executors
}
criterion_main!(benches);
