use crossbeam::channel::{bounded, unbounded, Receiver, Sender};
use ecs::pool::ThreadPool;
use ecs::scheduling::Unordered;
use ecs::{Application, Read, Write};
use scheduler::schedule_dag::PrecedenceGraph;
use std::collections::HashMap;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct A(i32);

#[derive(Eq, Ord, PartialOrd, PartialEq, Debug, Hash)]
enum TestSystem {
    Read,
    Write,
}

type InstantMessage = (TestSystem, Instant);

fn set_up_read_and_write_timespan_verifier(
    instant_reporter0: Sender<InstantMessage>,
    instant_reports0: Receiver<InstantMessage>,
) -> (Application, Receiver<()>, JoinHandle<()>) {
    let instant_reporter1 = instant_reporter0.clone();

    let read_system = move |_: Read<A>| {
        instant_reporter0
            .send((TestSystem::Read, Instant::now()))
            .map_err(|e| eprintln!("error: {e}"))
            .unwrap();
        thread::sleep(Duration::from_nanos(100));
        instant_reporter0
            .send((TestSystem::Read, Instant::now()))
            .map_err(|e| eprintln!("error: {e}"))
            .unwrap();
    };

    let write_system = move |_: Write<A>| {
        instant_reporter1
            .send((TestSystem::Write, Instant::now()))
            .map_err(|e| eprintln!("error: {e}"))
            .unwrap();
        thread::sleep(Duration::from_nanos(100));
        instant_reporter1
            .send((TestSystem::Write, Instant::now()))
            .map_err(|e| eprintln!("error: {e}"))
            .unwrap();
    };

    let (shutdown_sender, shutdown_receiver) = unbounded();
    let monitoring_thread = thread::spawn(move || {
        let mut thread_time_reports = HashMap::new();
        for _ in 0..4 {
            let (system, instant) = instant_reports0.recv().unwrap();
            let time_reports = thread_time_reports.entry(system).or_insert_with(Vec::new);
            time_reports.push(instant);
        }
        drop(shutdown_sender);

        let mut read_start = None;
        let mut read_end = None;
        let mut write_start = None;
        let mut write_end = None;
        for thread in thread_time_reports.keys() {
            let time_reports = thread_time_reports.get(thread).unwrap();
            match thread {
                TestSystem::Read => {
                    read_start.replace(time_reports[0]);
                    read_end.replace(time_reports[1]);
                }
                TestSystem::Write => {
                    write_start.replace(time_reports[0]);
                    write_end.replace(time_reports[1]);
                }
            }
        }
        assert!(read_start.unwrap() < write_start.unwrap());
        assert!(read_end.unwrap() < write_start.unwrap());
        assert!(write_start.unwrap() > read_end.unwrap());
        assert!(write_end.unwrap() > read_end.unwrap());
    });

    let mut app = Application::default()
        .add_system(read_system)
        .add_system(write_system);
    let entity = app.new_entity();
    app.add_component_to_entity(entity, A(1));

    (app, shutdown_receiver, monitoring_thread)
}

#[test]
fn reads_and_writes_of_same_component_do_not_execute_concurrently_in_correct_schedule() {
    let (instant_reporter, instant_reports) = bounded(4);
    #[allow(clippy::redundant_clone)]
    // Clone required to keep channel alive until systems have run.
    let instant_reports0 = instant_reports.clone();
    let (mut app, shutdown_receiver, monitoring_thread) =
        set_up_read_and_write_timespan_verifier(instant_reporter, instant_reports0);

    app.run::<ThreadPool, PrecedenceGraph>(shutdown_receiver);
    monitoring_thread.join().unwrap();
}

#[test]
#[should_panic]
fn reads_and_writes_of_same_component_execute_concurrently_in_incorrect_schedule() {
    let (instant_reporter, instant_reports) = bounded(4);
    #[allow(clippy::redundant_clone)]
    // Clone required to keep channel alive until systems have run.
    let instant_reports0 = instant_reports.clone();
    let (mut app, shutdown_receiver, monitoring_thread) =
        set_up_read_and_write_timespan_verifier(instant_reporter, instant_reports0);

    app.run::<ThreadPool, Unordered>(shutdown_receiver);
    monitoring_thread.join().unwrap();
}
