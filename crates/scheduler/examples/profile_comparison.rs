use crossbeam::channel::{unbounded, Receiver};
use ecs::pool::ThreadPool;
use ecs::scheduler_rayon::RayonChaos;
use ecs::scheduling::{Schedule, ScheduleExecutor};
use ecs::Application;
use scheduler::schedule_dag::PrecedenceGraph;
use std::thread;
use std::time::Duration;

fn create_application(number_of_systems: i32) -> (Application, Receiver<()>) {
    let mut app = Application::default();

    let (count_sender, count_receiver) = unbounded();

    for i in 0..number_of_systems {
        let count_sender = count_sender.clone();
        let sleep_ms = i;
        let independent_system = move || {
            let _span = tracing_tracy::client::span!("independent_system");
            thread::sleep(Duration::from_millis(sleep_ms as u64));
            count_sender.send(()).unwrap();
        };
        app = app.add_system(independent_system);
    }

    (app, count_receiver)
}

fn run_application_until_system_execution_count<'a, E, S>(
    app: &'a mut Application,
    count_receiver: Receiver<()>,
    up_till_count: u64,
) where
    E: ScheduleExecutor<'a> + Default,
    S: Schedule<'a>,
{
    let (shutdown_sender, shutdown_receiver) = unbounded();

    let _completion_thread = thread::spawn(move || {
        for _ in 0..up_till_count {
            count_receiver.recv().unwrap();
        }
        drop(shutdown_sender);
    });

    app.run::<E, S>(shutdown_receiver);
}

fn main() {
    use tracing_subscriber::layer::SubscriberExt;
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(tracing_tracy::TracyLayer::new()),
    )
    .unwrap();

    let exit_after_system_executions = 10_000;
    let number_of_systems = 38;

    let (mut app, count_receiver) = create_application(number_of_systems);
    run_application_until_system_execution_count::<RayonChaos, PrecedenceGraph>(
        &mut app,
        #[allow(clippy::redundant_clone)] // Needed to keep channel alive long enough.
        count_receiver.clone(),
        exit_after_system_executions,
    );

    let (mut app, count_receiver) = create_application(number_of_systems);
    run_application_until_system_execution_count::<ThreadPool, PrecedenceGraph>(
        &mut app,
        #[allow(clippy::redundant_clone)] // Needed to keep channel alive long enough.
        count_receiver.clone(),
        exit_after_system_executions,
    );
}
