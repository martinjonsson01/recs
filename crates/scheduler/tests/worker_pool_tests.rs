use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder, IntoTickable, Tickable};
//noinspection RsUnusedImport - CLion can't recognize that `Write` is being used.
use ecs::systems::Write;
//noinspection RsUnusedImport - CLion can't recognize that `timeout` is being used.
use ntest::timeout;
use scheduler::executor::*;
use scheduler::schedule::PrecedenceGraph;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{iter, thread};
use tracing::debug;

#[test_log::test]
#[timeout(1000)]
fn scheduler_runs_application() {
    #[derive(Debug)]
    struct TestComponent(i32);

    static HAS_RUN: AtomicBool = AtomicBool::new(false);
    let verify_run_system = move || {
        debug!("verify_run_system has run!");

        // Shut down application once this has run.
        HAS_RUN.store(true, Ordering::SeqCst);
    };

    fn system_with_read_and_write(mut health: Write<TestComponent>) {
        debug!("System with one mutable parameter {:?} .. ", health);
        *health = TestComponent(99);
        debug!("mutated to {:?}!", health);
    }

    let mut application = BasicApplicationBuilder::default()
        .add_system(verify_run_system)
        .add_system(system_with_read_and_write)
        .build();

    let entity0 = application.create_entity().unwrap();
    let entity1 = application.create_entity().unwrap();
    application
        .add_component(entity0, TestComponent(100))
        .unwrap();
    application
        .add_component(entity1, TestComponent(43))
        .unwrap();

    let runner = application
        .into_tickable::<WorkerPool, PrecedenceGraph>()
        .unwrap();
    while !HAS_RUN.load(Ordering::SeqCst) {
        runner.tick().unwrap();
    }
}

fn run_application_with_fake_systems(
    expected_executions: u8,
    system_execution_times: Vec<Duration>,
) -> Vec<u8> {
    let system_execution_counts: Vec<_> = iter::repeat_with(|| AtomicU8::new(0))
        .take(system_execution_times.len())
        .collect();

    let system_execution_counts = Arc::new(system_execution_counts);
    let systems = system_execution_times
        .into_iter()
        .enumerate()
        .map(|(i, execution_time)| {
            let system_execution_counts_ref = Arc::clone(&system_execution_counts);
            move || {
                debug!("  Running system {i}...");
                thread::sleep(execution_time);

                system_execution_counts_ref[i]
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| Some(count + 1))
                    .unwrap();
                let executions = system_execution_counts_ref[i].load(Ordering::SeqCst);
                debug!("  Finished system {i} for the {executions}:th time!");
            }
        });

    let application = BasicApplicationBuilder::default()
        .add_systems(systems)
        .build();

    let runner = application
        .into_tickable::<WorkerPool, PrecedenceGraph>()
        .unwrap();
    for _ in 0..expected_executions {
        runner.tick().unwrap();
    }

    system_execution_counts
        .iter()
        .map(|atomic_i8| atomic_i8.load(Ordering::SeqCst))
        .collect()
}

macro_rules! assert_approx_eq {
    ($a:expr, $b:expr, $message:expr) => {
        let diff = approx::AbsDiff::default().epsilon(1);
        assert!(
            diff.eq(&$a, &$b),
            "{}\n\n\t{}  = {:?}\n\t{} = {:?}\n\n",
            format!($message),
            stringify!($a),
            $a,
            stringify!($b),
            $b,
        )
    };
}

#[test_log::test]
#[timeout(1000)]
fn scheduler_runs_systems_same_number_of_times() {
    let expected_executions = 4;
    let systems_count = 10;
    let system_durations: Vec<_> = (0..systems_count).map(Duration::from_micros).collect();

    let system_execution_counts =
        run_application_with_fake_systems(expected_executions, system_durations);

    let expected_executions = system_execution_counts[0];

    for actual_executions in system_execution_counts {
        // Allow a difference of 1 execution, because when initiating shutdown a task may
        // already have begun its execution, and it won't be interrupted during the execution.
        assert_approx_eq!(
            actual_executions,
            expected_executions,
            "systems have to run approximately the same number of times (±1)"
        );
    }
}

#[test_log::test]
#[timeout(1000)]
fn scheduler_runs_systems_a_set_amount_of_times() {
    let systems_count = 10;
    let expected_executions = 4;
    let system_durations: Vec<_> = (0..systems_count).map(Duration::from_micros).collect();

    let system_execution_counts =
        run_application_with_fake_systems(expected_executions, system_durations);

    for actual_executions in system_execution_counts {
        // Allow a difference of 1 execution, because when initiating shutdown a task may
        // already have begun its execution, and it won't be interrupted during the execution.
        assert_approx_eq!(
            actual_executions,
            expected_executions,
            "systems should run approximately {expected_executions}±1 times"
        );
    }
}
