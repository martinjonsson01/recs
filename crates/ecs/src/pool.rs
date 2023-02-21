use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::thread::{Scope, ScopedJoinHandle};
use std::time::Duration;
use std::{iter, thread};

use crossbeam::channel::{Receiver, TryRecvError};
use crossbeam::deque::{Injector, Stealer, Worker};

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
}

macro_rules! thread_println {
    ($($input:expr),*) => {
        let mut stdout = std::io::stdout().lock();

        let prefix = format!("{:?}: ", thread::current().id());
        std::io::Write::write_all(&mut stdout, prefix.as_bytes())
            .expect("stdio should be accessible");

        let formatted_text = format!($($input),*);
        std::io::Write::write_all(&mut stdout, formatted_text.as_bytes())
            .expect("stdio should be accessible");

        std::io::Write::write_all(&mut stdout, b"\n")
            .expect("stdio should be accessible");

        std::io::Write::flush(&mut stdout)
            .expect("there shouldn't be any I/O errors when writing normal text");
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
            let workers: Vec<_> = (0..num_cpus::get())
                .map(|_thread_number| {
                    let injector = Arc::clone(&self.injector);
                    WorkerThread::start(scope, shutdown_receiver.clone(), injector, vec![])
                })
                .collect();

            for system in systems.iter_mut() {
                let task = move || {
                    thread_println!("working on {:?} and {:?}", system, world);
                    system.run(world);
                };
                self.injector.push(Task::new(task));
            }

            for worker in workers {
                let id = worker.thread().id();
                worker.join().expect("Worker thread shouldn't panic");
                thread_println!("thread id {:?} has exited ", id);
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
    local_queue: Worker<Task<'env>>,
    global_queue: Arc<Injector<Task<'env>>>,
    stealers: Vec<Stealer<Task<'env>>>,
}

impl<'scope, 'env: 'scope> WorkerThread<'env> {
    fn start(
        scope: &'scope Scope<'scope, '_>,
        shutdown_receiver: Receiver<()>,
        global_queue: Arc<Injector<Task<'env>>>,
        stealers: Vec<Stealer<Task<'env>>>,
    ) -> ScopedJoinHandle<'scope, ()> {
        Self::start_with_tasks(scope, shutdown_receiver, global_queue, stealers, vec![])
    }

    fn start_with_tasks(
        scope: &'scope Scope<'scope, '_>,
        shutdown_receiver: Receiver<()>,
        global_queue: Arc<Injector<Task<'env>>>,
        stealers: Vec<Stealer<Task<'env>>>,
        tasks: impl IntoIterator<Item = Task<'env>> + Send + 'scope,
    ) -> ScopedJoinHandle<'scope, ()> {
        scope.spawn(move || {
            let worker = Self {
                shutdown_receiver,
                local_queue: Worker::new_fifo(),
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
                thread_println!("found no task");
                // todo: put thread to sleep until new tasks arrive - don't just sleep for 10ms
                thread::sleep(Duration::from_millis(10));
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
    use std::sync::Arc;
    use std::thread;

    use crossbeam::atomic::AtomicCell;
    use crossbeam::channel::{bounded, unbounded};
    use crossbeam::sync::Parker;
    use itertools::Itertools;
    use rand::Rng;

    use crate::{Application, Write};

    use super::*;

    type MockTask<'env> = Option<Task<'env>>;

    fn mock_global_queue<'env>() -> Arc<Injector<Task<'env>>> {
        Default::default()
    }

    // Generates a unique mock task.
    fn mock_task<'env>() -> MockTask<'env> {
        let random = rand::thread_rng().gen::<i32>();
        Some(Task::new(move || println!("{random}")))
    }

    fn mock_worker<'env>(
        global_queue: Injector<Task<'env>>,
        stealers: Vec<Stealer<Task<'env>>>,
    ) -> WorkerThread<'env> {
        let (_, shutdown_receiver) = bounded(1);
        WorkerThread {
            shutdown_receiver,
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
        let stealers = vec![other_worker.stealer()];
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

    #[test]
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
    fn worker_runs_given_task() {
        let parker = Parker::new();
        let unparker = parker.unparker().clone();

        let has_task_run = Arc::new(AtomicCell::new(false));
        let has_task_run_clone = Arc::clone(&has_task_run);
        let task = Task::new(|| {
            thread_println!("task executing");
            has_task_run_clone.store(true);
            unparker.unpark();
        });

        thread::scope(|scope| {
            let (shutdown_sender, shutdown_receiver) = bounded(1);
            WorkerThread::start_with_tasks(
                scope,
                shutdown_receiver,
                mock_global_queue(),
                vec![],
                vec![task],
            );
            // Wait for task to complete
            parker.park_timeout(Duration::from_secs(1));
            shutdown_sender.send(()).unwrap();

            assert!(has_task_run.take())
        })
    }

    #[test]
    fn worker_runs_on_another_thread() {
        let parker = Parker::new();
        let unparker = parker.unparker().clone();

        let worker_thread_id = Arc::new(AtomicCell::new(None));
        let worker_thread_id_clone = Arc::clone(&worker_thread_id);
        let task = Task::new(|| {
            println!("task 12");
            worker_thread_id_clone.store(Some(thread::current().id()));
            unparker.unpark();
        });

        thread::scope(|scope| {
            let (shutdown_sender, shutdown_receiver) = bounded(1);
            WorkerThread::start_with_tasks(
                scope,
                shutdown_receiver,
                mock_global_queue(),
                vec![],
                vec![task],
            );
            // Wait for task to complete
            parker.park_timeout(Duration::from_secs(1));
            shutdown_sender.send(()).unwrap();

            let main_thread_id = thread::current().id();
            assert_ne!(
                Some(main_thread_id),
                worker_thread_id.take(),
                "worker shouldn't run on main thread"
            )
        })
    }

    #[test]
    fn worker_runs_multiple_tasks() {
        let parker = Parker::new();
        let unparker = Arc::new(parker.unparker().clone());

        let mut tasks = vec![];
        let mut have_tasks_run = vec![];

        let task_count = 2;
        for i in 0..task_count {
            let has_task_run = Arc::new(AtomicCell::new(false));
            let has_task_run_clone = Arc::clone(&has_task_run);
            let unparker_clone = Arc::clone(&unparker);

            let task = Task::new(move || {
                println!("{i}");
                has_task_run_clone.store(true);
                // Last task to run unparks
                if i == task_count - 1 {
                    unparker_clone.unpark();
                }
            });

            tasks.push(task);
            have_tasks_run.push(has_task_run);
        }

        thread::scope(|scope| {
            let (shutdown_sender, shutdown_receiver) = bounded(1);
            WorkerThread::start_with_tasks(
                scope,
                shutdown_receiver,
                mock_global_queue(),
                vec![],
                tasks,
            );
            // Wait for last task to complete
            parker.park_timeout(Duration::from_secs(1));
            for _ in 0..task_count {
                shutdown_sender.send(()).unwrap();
            }

            let all_tasks_ran = vec![true].repeat(task_count as usize);
            let have_tasks_run: Vec<_> = have_tasks_run
                .iter()
                .map(|has_run| has_run.take())
                .collect();
            assert_eq!(all_tasks_ran, have_tasks_run, "all tasks should run")
        })
    }

    #[test]
    fn worker_runs_tasks_on_same_thread() {
        let parker = Parker::new();
        let unparker = Arc::new(parker.unparker().clone());

        let mut tasks = vec![];
        let mut thread_ids = vec![];
        let task_count = 2;
        for i in 0..task_count {
            let worker_thread_id = Arc::new(AtomicCell::new(None));
            let worker_thread_id_clone = Arc::clone(&worker_thread_id);
            let unparker_clone = Arc::clone(&unparker);

            let task = Task::new(move || {
                let id = thread::current().id();
                println!("thread {id:?}: {i}");
                worker_thread_id_clone.store(Some(id));
                // Last task to run unparks
                if i == task_count - 1 {
                    unparker_clone.unpark();
                }
            });
            tasks.push(task);
            thread_ids.push(Arc::clone(&worker_thread_id));
        }

        thread::scope(|scope| {
            let (shutdown_sender, shutdown_receiver) = bounded(1);
            WorkerThread::start_with_tasks(
                scope,
                shutdown_receiver,
                mock_global_queue(),
                vec![],
                tasks,
            );
            // Wait for last task to complete
            parker.park_timeout(Duration::from_secs(1));
            for _ in 0..task_count {
                shutdown_sender.send(()).unwrap();
            }

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
        })
    }

    #[test]
    fn threadpool_scheduler_runs_application() {
        #[derive(Debug)]
        struct TestComponent(i32);

        let (shutdown_sender, shutdown_receiver) = unbounded();
        let verify_run_system = move || {
            thread_println!("verify_run_system has run!");

            // Shut down application once this has run.
            for _ in 0..num_cpus::get() {
                match shutdown_sender.try_send(()) {
                    Ok(_) => {
                        thread_println!("sent shut down!");
                    }
                    Err(e) => {
                        thread_println!("could not send shutdown because of {e:?}");
                    }
                }
            }
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
    }
}
