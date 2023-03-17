use crate::executor::Task;
use crossbeam::channel::{Receiver, Sender, TryRecvError};
use crossbeam::deque::{Injector, Stealer, Worker};
use crossbeam::sync::Parker;
use std::sync::Arc;
use std::thread::{Builder, Scope, ScopedJoinHandle};
use std::{iter, panic, thread};
use tracing::{debug, error, info};

#[derive(Debug)]
pub(super) struct WorkerHandle<'scope> {
    /// Store the join handle in an Option so it can be extracted from the
    /// Option in a Drop-implementation without having ownership of the struct.
    thread: Option<ScopedJoinHandle<'scope, ()>>,
}

impl<'scope> Drop for WorkerHandle<'scope> {
    fn drop(&mut self) {
        if let Some(inner) = self.thread.take() {
            let worker_name = inner.thread().name().unwrap_or("Unnamed worker").to_owned();

            if thread::panicking() {
                let current = thread::current();
                let current_thread = current.name().unwrap_or("Thread");
                error!(
                    "{current_thread} has panicked, so we're not waiting for {worker_name} to finish",
                );
                return;
            }

            debug!("Waiting for {} to finish...", worker_name);
            let res = inner.join();
            debug!(
                ".. {} terminated with {}",
                worker_name,
                if res.is_ok() { "ok" } else { "err" }
            );

            // Escalate panic while avoiding aborting the process.
            if let Err(e) = res {
                panic::resume_unwind(e);
            }
        }
    }
}

pub(super) struct WorkerBuilder<'task> {
    shutdown_receiver: Receiver<()>,
    parker: Parker,
    local_queue: Worker<Task<'task>>,
    global_queue: Arc<Injector<Task<'task>>>,
    stealers: Arc<Vec<Stealer<Task<'task>>>>,
    name: Option<String>,
    initial_tasks: Vec<Task<'task>>,
}

impl<'scope, 'task: 'scope> WorkerBuilder<'task> {
    pub(super) fn new(
        shutdown_receiver: Receiver<()>,
        parker: Parker,
        local_queue: Worker<Task<'task>>,
        global_queue: Arc<Injector<Task<'task>>>,
        stealers: Arc<Vec<Stealer<Task<'task>>>>,
    ) -> Self {
        Self {
            shutdown_receiver,
            parker,
            local_queue,
            global_queue,
            stealers,
            name: None,
            initial_tasks: vec![],
        }
    }

    pub(super) fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub(super) fn start(
        self,
        scope: &'scope Scope<'scope, 'task>,
        panic_guard: Sender<()>,
    ) -> WorkerHandle<'scope> {
        let WorkerBuilder {
            shutdown_receiver,
            parker,
            local_queue,
            global_queue,
            stealers,
            name,
            initial_tasks,
        } = self;

        let mut thread = Builder::new();
        if let Some(name) = name {
            thread = thread.name(name);
        }

        let thread = thread
            .spawn_scoped(scope, || {
                // Move panic_guard into thread so it will be dropped when thread exits/panics.
                let _panic_guard = panic_guard;

                // Worker has to be created inside thread, because some of its contents aren't Send.
                let worker = WorkerThread {
                    shutdown_receiver,
                    parker,
                    local_queue,
                    global_queue,
                    stealers,
                };
                for initial_task in initial_tasks {
                    worker.local_queue.push(initial_task);
                }
                worker.run();
            })
            .expect("Thread name should not contain null bytes");

        WorkerHandle {
            thread: Some(thread),
        }
    }
}

#[derive(Debug)]
struct WorkerThread<'task> {
    /// How the worker is informed to shut down.
    shutdown_receiver: Receiver<()>,
    /// Used by the worker to go to sleep (i.e. park) when there is no work to do.
    parker: Parker,
    /// A local queue of work to be done by this thread (which can be stolen by other threads).
    local_queue: Worker<Task<'task>>,
    /// A reference to the global task queue, to fetch new tasks from.
    global_queue: Arc<Injector<Task<'task>>>,
    /// Stealer-references to peer worker threads, so this thread can steal from them.
    stealers: Arc<Vec<Stealer<Task<'task>>>>,
}

impl<'task> WorkerThread<'task> {
    #[tracing::instrument(skip(self))]
    fn run(&self) {
        while let Err(TryRecvError::Empty) = self.shutdown_receiver.try_recv() {
            if let Some(task) = self.find_task() {
                debug!("executing task...");
                task.run();
            } else {
                debug!("found no task, going to sleep...");
                self.parker.park();
            }
        }
        info!("exited due to shutdown command!");
    }

    #[tracing::instrument]
    fn find_task(&self) -> Option<Task<'task>> {
        self.local_queue.pop().or_else(|| {
            // Repeat while the queues return `Steal::Retry`...
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
    use super::*;
    use crossbeam::atomic::AtomicCell;
    use crossbeam::channel::bounded;
    use itertools::Itertools;
    use ntest::timeout;
    use std::thread;
    use std::time::Duration;
    use test_log::test;

    type MockTask<'env> = Option<Task<'env>>;

    fn mock_global_queue<'env>() -> Arc<Injector<Task<'env>>> {
        Default::default()
    }

    // Generates a unique mock task.
    fn mock_task<'env>() -> MockTask<'env> {
        Some(Task::new(move || {
            use rand::Rng;
            debug!("{}", rand::thread_rng().gen::<i32>());
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
        workers_task_id: Option<usize>,
        others_task_id: Option<usize>,
        globals_task_id: Option<usize>,
    }

    /// Sets up a simple scenario:
    ///
    /// there are two workers: worker       and     other
    /// with tasks:            workers_task and     others_task
    ///
    /// there's also a task in the global queue: globals_task.
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

    impl<'task> WorkerBuilder<'task> {
        fn with_tasks(mut self, tasks: Vec<Task<'task>>) -> Self {
            self.initial_tasks = tasks;
            self
        }
    }

    fn spawn_worker_and_wait_for_task_completion(task_functions: Vec<impl FnMut() + Send>) {
        let delayed_functions: Vec<fn()> = vec![];
        spawn_worker_and_wait_for_task_completion_with_delay(task_functions, delayed_functions);
    }

    /// [`task_functions`] are given to the worker as initial tasks.
    /// [`delayed_task_functions`] are put in tasks that get fed to the global worker queue
    /// after a slight delay.
    fn spawn_worker_and_wait_for_task_completion_with_delay(
        task_functions: Vec<impl FnMut() + Send>,
        delayed_task_functions: Vec<impl FnMut() + Send>,
    ) {
        let task_count = task_functions.len();
        let mut tasks = vec![];
        // To wait for all tasks to complete before returning.
        let mut task_completion_parkers = vec![];
        // To check whether all tasks have run.
        let mut have_tasks_run = vec![];

        // Create tasks that call the given `task_functions`, but also add in the necessary
        // infrastructure to be able to monitor whether the task has been completed.
        for mut task_function in task_functions {
            // Used by the test thread to check whether tasks have executed.
            let has_task_run = Arc::new(AtomicCell::new(false));
            let has_task_run_clone = Arc::clone(&has_task_run);

            // Used by the task to signal to any waiting test-thread that it has now run.
            let task_completion_parker = Parker::new();
            let task_completion_unparker = task_completion_parker.unparker().clone();

            let task = Task::new(move || {
                task_function();
                has_task_run_clone.store(true);
                task_completion_unparker.unpark();
            });

            tasks.push(task);
            task_completion_parkers.push(task_completion_parker);
            have_tasks_run.push(has_task_run);
        }

        let worker_parker = Parker::new();
        let worker_notify_new_task = worker_parker.unparker().clone();
        let worker_notify_shutdown = worker_parker.unparker().clone();

        thread::scope(|scope| {
            let (shutdown_sender, shutdown_receiver) = bounded(1);

            let global_queue = mock_global_queue();
            let (panic_guard_sender, _) = bounded(1);
            let _worker = WorkerBuilder::new(
                shutdown_receiver,
                worker_parker,
                Worker::new_fifo(),
                Arc::clone(&global_queue),
                Arc::new(vec![]),
            )
            .with_name("Mock worker".to_string())
            .with_tasks(tasks)
            .start(scope, panic_guard_sender);

            scope.spawn(move || {
                thread::sleep(Duration::from_millis(10));
                debug!("Pushing delayed tasks!");
                for delayed_task_function in delayed_task_functions {
                    global_queue.push(Task::new(delayed_task_function));
                    worker_notify_new_task.unpark();
                }
            });

            // Wait for all tasks to complete.
            for task_parker in task_completion_parkers {
                task_parker.park_timeout(Duration::from_secs(1));
            }
            drop(shutdown_sender);
            worker_notify_shutdown.unpark(); // Wake worker up so it can shut down.

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
    fn worker_runs_given_task() {
        let task = || {
            debug!("task executing");
        };

        spawn_worker_and_wait_for_task_completion(vec![task]);
    }

    #[test]
    #[timeout(1000)]
    fn worker_runs_on_another_thread() {
        let worker_thread_id = Arc::new(AtomicCell::new(None));
        let worker_thread_id_clone = Arc::clone(&worker_thread_id);

        let task = || {
            debug!("task executing");
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
            .map(|i| {
                move || {
                    debug!("task {i} executing");
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
            .map(|i| {
                let worker_thread_id = Arc::new(AtomicCell::new(None));
                let worker_thread_id_clone = Arc::clone(&worker_thread_id);
                thread_ids.push(Arc::clone(&worker_thread_id));

                move || {
                    let id = thread::current().id();
                    debug!("thread {id:?}: {i}");
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
            debug!("delayed task is running");
        };

        spawn_worker_and_wait_for_task_completion_with_delay(task_functions, vec![delayed_task]);
    }
}
