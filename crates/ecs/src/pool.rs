use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::thread::{Scope, ScopedJoinHandle};
use std::{iter, thread};

use crossbeam::channel::{Receiver, TryRecvError};
use crossbeam::deque::{Injector, Stealer, Worker};
use crossbeam::sync::{Parker, Unparker};

use crate::{Schedule, System, World};

struct Task<'a> {
    uid: u64,
    function: Box<dyn FnMut() + Send + 'a>,
}

impl<'a> Debug for Task<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Task<>")
    }
}

impl<'a> Default for Task<'a> {
    fn default() -> Self {
        Task::new(|| {})
    }
}

impl<'a> PartialEq for Task<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}

impl<'a> Task<'a> {
    fn new(function: impl FnMut() + Send + 'a) -> Self {
        Self {
            uid: 0,
            function: Box::new(function),
        }
    }

    fn run(&mut self) {
        (self.function)()
    }
}

#[derive(Debug, Default)]
pub struct ThreadPool<'a> {
    injector: Arc<Injector<Task<'a>>>,
    worker_unparkers: Vec<Unparker>,
}

/// When multiple tests are run concurrently, using this macro causes deadlocks
/// since they will all try to lock stdout concurrently. Therefore this
/// macro does nothing unless the cfg option 'print_threads' is enabled.
///
/// The run configuration "Test all (print from threads)" runs test sequentially
/// and enables the printing inside thread_println!.
#[macro_export]
macro_rules! thread_println {
    ($($input:expr),*) => {
        #[cfg(print_threads)]
        {
            let mut stdout = std::io::stdout().lock();

            let prefix = format!("{:?}:\t", thread::current().id());
            std::io::Write::write_all(&mut stdout, prefix.as_bytes())
                .expect("stdio should be accessible");

            let formatted_text = format!($($input),*);
            std::io::Write::write_all(&mut stdout, formatted_text.as_bytes())
                .expect("stdio should be accessible");

            std::io::Write::write_all(&mut stdout, b"\n")
                .expect("stdio should be accessible");

            std::io::Write::flush(&mut stdout)
                .expect("there shouldn't be any I/O errors when writing normal text");

            drop(stdout);
        }
    }
}

impl<'a> ThreadPool<'a> {
    fn add_task(&mut self, task: Task<'a>) {
        self.injector.push(task);
        self.notify_all_workers();
    }

    fn notify_all_workers(&self) {
        for unparker in &self.worker_unparkers {
            unparker.unpark();
        }
    }
}

impl<'a> Schedule<'a> for ThreadPool<'a> {
    fn execute(
        &mut self,
        systems: &'a mut Vec<Box<dyn System>>,
        world: &'a World,
        shutdown_receiver: Receiver<()>,
    ) {
        thread::scope(|scope| {
            let (local_queues, stealers): (Vec<_>, Vec<_>) = (0..num_cpus::get())
                .map(|_| {
                    let local_queue = Worker::new_fifo();
                    let stealer = local_queue.stealer();
                    (local_queue, stealer)
                })
                .unzip();
            let stealers = Arc::new(stealers);

            let workers: Vec<_> = local_queues
                .into_iter()
                .map(|local_queue| {
                    let injector = Arc::clone(&self.injector);
                    let parker = Parker::new();
                    let unparker = parker.unparker().clone();
                    self.worker_unparkers.push(unparker);

                    WorkerThread::start(
                        scope,
                        shutdown_receiver.clone(),
                        parker,
                        local_queue,
                        injector,
                        Arc::clone(&stealers),
                    )
                })
                .collect();

            // Keep dealing out tasks until shutdown command is received.
            while let Err(TryRecvError::Empty) = shutdown_receiver.try_recv() {
                for system in systems.iter() {
                    let task = move || {
                        thread_println!("working on {:?} and {:?}", system, world);
                        system.run(world);
                    };
                    self.add_task(Task::new(task));
                }
            }
            println!(
                "exiting with {:?} tasks in global queue...",
                self.injector.len()
            );

            // Wake up any sleeping workers so they can shut down.
            self.notify_all_workers();

            for worker in workers {
                let _id = worker.thread().id();
                worker.join().expect("Worker thread shouldn't panic");
                thread_println!("joined thread with id {_id:?} ");
            }
        });
        thread_println!("exited!");
    }
}

/// A sad, lowly, decrepit and downtrodden laborer who toils in the processor fields
/// day-in and day-out with no pay and zero happiness.
///
/// (A worker, but the name `Worker` is already taken by crossbeam's local queue struct.)
trait Peasant {
    type Task;

    fn find_task(&self) -> Option<Self::Task>;
}

#[derive(Debug)]
struct WorkerThread<'env> {
    shutdown_receiver: Receiver<()>,
    parker: Parker,
    local_queue: Worker<Task<'env>>,
    global_queue: Arc<Injector<Task<'env>>>,
    stealers: Arc<Vec<Stealer<Task<'env>>>>,
}

impl<'scope, 'env: 'scope> WorkerThread<'env> {
    fn start(
        scope: &'scope Scope<'scope, '_>,
        shutdown_receiver: Receiver<()>,
        parker: Parker,
        local_queue: Worker<Task<'env>>,
        global_queue: Arc<Injector<Task<'env>>>,
        stealers: Arc<Vec<Stealer<Task<'env>>>>,
    ) -> ScopedJoinHandle<'scope, ()> {
        Self::start_with_tasks(
            scope,
            shutdown_receiver,
            parker,
            local_queue,
            global_queue,
            stealers,
            vec![],
        )
    }

    fn start_with_tasks(
        scope: &'scope Scope<'scope, '_>,
        shutdown_receiver: Receiver<()>,
        parker: Parker,
        local_queue: Worker<Task<'env>>,
        global_queue: Arc<Injector<Task<'env>>>,
        stealers: Arc<Vec<Stealer<Task<'env>>>>,
        tasks: impl IntoIterator<Item = Task<'env>> + Send + 'scope,
    ) -> ScopedJoinHandle<'scope, ()> {
        scope.spawn(move || {
            let worker = Self {
                shutdown_receiver,
                parker,
                local_queue,
                global_queue,
                stealers,
            };
            for task in tasks {
                worker.local_queue.push(task);
            }
            worker.run();
        })
    }

    fn run(&self) {
        while let Err(TryRecvError::Empty) = self.shutdown_receiver.try_recv() {
            thread_println!("looping!");
            if let Some(mut task) = self.find_task() {
                thread_println!("running task...");
                task.run();
            } else {
                thread_println!("found no task, going to sleep...");
                self.parker.park();
            }
        }
        thread_println!("exited due to shutdown command!");
    }
}

impl<'env> Peasant for WorkerThread<'env> {
    type Task = Task<'env>;

    fn find_task(&self) -> Option<Self::Task> {
        self.local_queue.pop().or_else(|| {
            // Repeat while the queues return `Steal::Retry`
            iter::repeat_with(|| {
                self.global_queue
                    .steal_batch_and_pop(&self.local_queue)
                    .or_else(|| self.stealers.iter().map(|s| s.steal()).collect())
            })
            .find(|steal| !steal.is_retry())
            .and_then(|steal| steal.success())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI8, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crossbeam::atomic::AtomicCell;
    use crossbeam::channel::{bounded, unbounded};
    use crossbeam::sync::Parker;
    use itertools::Itertools;
    use ntest::timeout;
    #[cfg(print_threads)]
    use rand::Rng;

    use crate::{Application, Write};

    use super::*;

    type MockTask<'env> = Option<Task<'env>>;

    fn mock_global_queue<'env>() -> Arc<Injector<Task<'env>>> {
        Default::default()
    }

    // Generates a unique mock task.
    fn mock_task<'env>() -> MockTask<'env> {
        Some(Task::new(move || {
            thread_println!("{}", rand::thread_rng().gen::<i32>());
        }))
    }

    fn mock_worker<'env>(
        global_queue: Injector<Task<'env>>,
        stealers: Arc<Vec<Stealer<Task<'env>>>>,
    ) -> WorkerThread<'env> {
        let (_, shutdown_receiver) = bounded(1);
        WorkerThread {
            shutdown_receiver,
            parker: Default::default(),
            local_queue: Worker::new_fifo(),
            global_queue: Arc::new(global_queue),
            stealers,
        }
    }

    struct MockedSystem<'env> {
        worker: WorkerThread<'env>,
        workers_task_id: Option<u64>,
        others_task_id: Option<u64>,
        globals_task_id: Option<u64>,
    }

    /// Sets up a simple scenario:
    /// there are two workers: worker       and other
    /// with tasks:            workers_task and others_task
    /// and a task in global queue: globals_task
    fn mock_workers<'env>(
        workers_task: MockTask<'env>,
        others_task: MockTask<'env>,
        globals_task: MockTask<'env>,
    ) -> MockedSystem<'env> {
        let mut workers_task_id = None;
        let mut others_task_id = None;
        let mut globals_task_id = None;
        let global_queue = Injector::new();
        if let Some(globals_task) = globals_task {
            globals_task_id = Some(globals_task.uid);
            global_queue.push(globals_task)
        }
        let other_worker = Worker::new_fifo();
        if let Some(others_task) = others_task {
            others_task_id = Some(others_task.uid);
            other_worker.push(others_task)
        }
        let stealers = Arc::new(vec![other_worker.stealer()]);
        let worker = mock_worker(global_queue, stealers);
        if let Some(workers_task) = workers_task {
            workers_task_id = Some(workers_task.uid);
            worker.local_queue.push(workers_task)
        }
        MockedSystem {
            worker,
            workers_task_id,
            others_task_id,
            globals_task_id,
        }
    }

    fn spawn_worker_and_wait_for_task_completion(task_functions: Vec<impl FnMut() + Send>) {
        let delayed_functions: Vec<fn()> = vec![];
        spawn_worker_and_wait_for_task_completion_with_delay(task_functions, delayed_functions);
    }

    fn spawn_worker_and_wait_for_task_completion_with_delay(
        task_functions: Vec<impl FnMut() + Send>,
        delayed_task_functions: Vec<impl FnMut() + Send>,
    ) {
        let task_count = task_functions.len();
        let mut tasks = vec![];
        let mut task_parkers = vec![];
        let mut have_tasks_run = vec![];

        for mut task_function in task_functions {
            let has_task_run = Arc::new(AtomicCell::new(false));
            let has_task_run_clone = Arc::clone(&has_task_run);

            let task_parker = Parker::new();
            let task_unparker = task_parker.unparker().clone();

            let task = Task::new(move || {
                task_function();
                has_task_run_clone.store(true);
                task_unparker.unpark();
            });

            tasks.push(task);
            task_parkers.push(task_parker);
            have_tasks_run.push(has_task_run);
        }

        let worker_parker = Parker::new();
        let worker_unparker0 = worker_parker.unparker().clone();
        let worker_unparker1 = worker_parker.unparker().clone();

        thread::scope(|scope| {
            let (shutdown_sender, shutdown_receiver) = bounded(1);
            let global_queue = mock_global_queue();
            WorkerThread::start_with_tasks(
                scope,
                shutdown_receiver,
                worker_parker,
                Worker::new_fifo(),
                Arc::clone(&global_queue),
                Arc::new(vec![]),
                tasks,
            );

            scope.spawn(move || {
                thread::sleep(Duration::from_millis(10));
                thread_println!("Pushing delayed tasks!");
                for delayed_task_function in delayed_task_functions {
                    global_queue.push(Task::new(delayed_task_function));
                    worker_unparker0.unpark();
                }
            });

            // Wait for all tasks to complete
            for task_parker in task_parkers {
                task_parker.park_timeout(Duration::from_secs(1));
            }
            drop(shutdown_sender);
            worker_unparker1.unpark(); // Wake worker up so it can shut down.

            let all_tasks_ran = vec![true].repeat(task_count);
            let have_tasks_run: Vec<_> = have_tasks_run
                .iter()
                .map(|has_run| has_run.take())
                .collect();
            assert_eq!(all_tasks_ran, have_tasks_run, "all tasks should run")
        })
    }

    #[test]
    #[timeout(1000)]
    fn worker_takes_task_from_local_queue_first() {
        let MockedSystem {
            worker,
            workers_task_id,
            ..
        } = mock_workers(mock_task(), mock_task(), mock_task());

        let task = worker.find_task().unwrap();

        assert_eq!(workers_task_id.unwrap(), task.uid);
    }

    #[test]
    #[timeout(1000)]
    fn worker_takes_global_task_when_local_queue_empty() {
        let MockedSystem {
            worker,
            globals_task_id,
            ..
        } = mock_workers(None, mock_task(), mock_task());

        let task = worker.find_task().unwrap();

        assert_eq!(globals_task_id.unwrap(), task.uid);
    }

    #[test]
    #[timeout(1000)]
    fn worker_steals_task_when_local_and_global_queue_empty() {
        let MockedSystem {
            worker,
            others_task_id,
            ..
        } = mock_workers(None, mock_task(), None);

        let task = worker.find_task().unwrap();

        assert_eq!(others_task_id.unwrap(), task.uid);
    }

    #[test]
    #[timeout(1000)]
    fn worker_runs_given_task() {
        let task = || {
            thread_println!("task executing");
        };

        spawn_worker_and_wait_for_task_completion(vec![task]);
    }

    #[test]
    #[timeout(1000)]
    fn worker_runs_on_another_thread() {
        let worker_thread_id = Arc::new(AtomicCell::new(None));
        let worker_thread_id_clone = Arc::clone(&worker_thread_id);

        let task = || {
            thread_println!("task executing");
            worker_thread_id_clone.store(Some(thread::current().id()));
        };

        spawn_worker_and_wait_for_task_completion(vec![task]);

        let main_thread_id = thread::current().id();
        assert_ne!(
            Some(main_thread_id),
            worker_thread_id.take(),
            "worker shouldn't run on main thread"
        )
    }

    #[test]
    #[timeout(1000)]
    fn worker_runs_multiple_tasks() {
        let tasks = (1..=3)
            .map(|_i| {
                move || {
                    thread_println!("task {_i} executing");
                }
            })
            .collect();

        spawn_worker_and_wait_for_task_completion(tasks);
    }

    #[test]
    #[timeout(1000)]
    fn worker_runs_tasks_on_same_thread() {
        let mut thread_ids = vec![];

        let tasks = (1..=3)
            .map(|_i| {
                let worker_thread_id = Arc::new(AtomicCell::new(None));
                let worker_thread_id_clone = Arc::clone(&worker_thread_id);
                thread_ids.push(Arc::clone(&worker_thread_id));

                move || {
                    let id = thread::current().id();
                    thread_println!("thread {id:?}: {_i}");
                    worker_thread_id_clone.store(Some(id));
                }
            })
            .collect();

        spawn_worker_and_wait_for_task_completion(tasks);

        let main_thread_id = thread::current().id();
        let thread_ids: Vec<_> = thread_ids.iter().map(|id| id.take()).collect();
        let product = thread_ids
            .clone()
            .into_iter()
            .cartesian_product(thread_ids.into_iter());
        for (thread_id, other_thread_id) in product {
            assert_ne!(
                Some(main_thread_id),
                thread_id,
                "worker shouldn't run on main thread"
            );
            assert_eq!(
                thread_id, other_thread_id,
                "worker should run all its tasks in same thread"
            );
        }
    }

    #[test]
    #[timeout(1000)]
    fn worker_wakes_up_and_executes_delayed_task() {
        let task_functions: Vec<fn()> = vec![];
        let delayed_task = || {
            thread_println!("delayed task is running");
        };

        spawn_worker_and_wait_for_task_completion_with_delay(task_functions, vec![delayed_task]);
    }

    #[test]
    #[timeout(1000)]
    fn threadpool_scheduler_runs_application() {
        #[derive(Debug)]
        struct TestComponent(i32);

        let parker = Parker::new();
        let unparker = parker.unparker().clone();

        let (shutdown_sender, shutdown_receiver) = unbounded();
        let shutdown_thread = thread::spawn(move || {
            thread_println!("Parking shutdown thread...");
            parker.park();
            thread_println!("Shutting down");
            drop(shutdown_sender);
        });

        let verify_run_system = move || {
            thread_println!("verify_run_system has run!");

            // Shut down application once this has run.
            unparker.unpark();
        };

        fn system_with_read_and_write(health: Write<TestComponent>) {
            thread_println!(
                "  Hello from system with one mutable parameter {:?} .. ",
                health.output
            );
            *health.output = TestComponent(99);
            thread_println!("mutated to {:?}!", health.output);
        }

        let mut application: Application = Application::default()
            .add_system(verify_run_system)
            .add_system(system_with_read_and_write);

        let entity0 = application.new_entity();
        let entity1 = application.new_entity();
        application.add_component_to_entity(entity0, TestComponent(100));
        application.add_component_to_entity(entity1, TestComponent(43));

        let scheduler = ThreadPool::default();
        application.run(scheduler, shutdown_receiver);
        shutdown_thread.join().unwrap();
    }

    #[test]
    #[timeout(1000)]
    fn threadpool_scheduler_runs_systems_in_ticks() {
        let systems_count = 10;
        let max_executions = 4;

        let (shutdown_parkers, (shutdown_unparkers, system_execution_counts)): (
            Vec<_>,
            (Vec<_>, Vec<_>),
        ) = (0..systems_count)
            .map(|_| {
                let parker = Parker::new();
                let unparker = parker.unparker().clone();
                let system_execution_count = AtomicI8::new(0);
                (parker, (unparker, system_execution_count))
            })
            .unzip();

        let (shutdown_sender, shutdown_receiver) = unbounded();
        let shutdown_thread = thread::spawn(move || {
            thread_println!("Parking shutdown thread...");
            for shutdown_parker in shutdown_parkers {
                shutdown_parker.park();
            }
            thread_println!("Shutting down");
            drop(shutdown_sender);
        });

        let system_execution_counts = Arc::new(system_execution_counts);
        let shutdown_unparkers = Arc::new(shutdown_unparkers);
        let systems = (0..systems_count).map(|i| {
            let system_execution_counts_ref = Arc::clone(&system_execution_counts);
            let shutdown_unparkers_ref = Arc::clone(&shutdown_unparkers);
            move || {
                thread_println!("  Running system {i}...");
                thread::sleep(Duration::from_millis(10));
                thread_println!("  Finished system {i}!");
                system_execution_counts_ref[i]
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| Some(count + 1))
                    .unwrap();
                let executions = system_execution_counts_ref[i].load(Ordering::SeqCst);
                if executions == max_executions {
                    thread_println!(
                        "  System {i} has run {max_executions} times, ready to shut down..."
                    );
                    shutdown_unparkers_ref[i].unpark();
                }
            }
        });

        let mut application: Application = Application::default().add_systems(systems);

        let scheduler = ThreadPool::default();
        application.run(scheduler, shutdown_receiver);
        shutdown_thread.join().unwrap();
    }
}
