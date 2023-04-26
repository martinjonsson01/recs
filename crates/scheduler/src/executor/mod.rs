//! Executors that can run systems according to a given `ecs::Schedule`
//! in different ways, such as on a thread pool.

pub use crate::executor::worker::{WorkerBuilder, WorkerHandle};
use crossbeam::channel::{bounded, Receiver, Sender, TryRecvError};
use crossbeam::deque::{Injector, Worker};
use crossbeam::sync::{Parker, Unparker};
use ecs::{
    ExecutionError, ExecutionResult, Executor, NewTickReaction, Schedule, ScheduleError,
    SystemExecutionGuard, World,
};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::Scope;
use tracing::{debug, error, info, instrument};

mod worker;

/// A single unit of work that can be executed once and is then completed.
pub struct Task<'a> {
    uid: usize,
    // Might be a good optimization to have RecurringTask that uses an FnMut later on?
    function: Box<dyn FnOnce() + Send + 'a>,
}

impl<'a> std::fmt::Debug for Task<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task").field("uid", &self.uid).finish()
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
    #[instrument(skip_all)]
    fn new(function: impl FnOnce() + Send + 'a) -> Self {
        static TASK_COUNT: AtomicUsize = AtomicUsize::new(0);
        // This might overflow and wrap if the application runs for long with many systems,
        // but that is okay because by then any tasks using the same IDs will be long gone.
        let old_task_count = TASK_COUNT.fetch_add(1, Ordering::SeqCst);
        Self {
            uid: old_task_count,
            function: Box::new(function),
        }
    }

    #[instrument(skip(self))]
    fn run(self) {
        (self.function)()
    }
}

/// Executes tasks on a set of workers running on separate threads.
#[derive(Debug)]
pub struct WorkerPool<'task> {
    /// The main work queue that workers will take tasks from.
    global_queue: Arc<Injector<Task<'task>>>,
    /// Used to construct workers during pool initialization.
    worker_builders: Vec<WorkerBuilder<'task>>,
    /// Handles to workers that can be used to wake them up, in case they were sleeping due to
    /// lack of tasks.
    worker_unparkers: Vec<Unparker>,
    /// Can be dropped to shut down all workers.
    worker_shutdown_sender: Option<Sender<()>>,
}

impl<'task> Default for WorkerPool<'task> {
    fn default() -> Self {
        let mut global_queue = Arc::new(Injector::new());
        let (worker_shutdown_sender, worker_builders, worker_unparkers) =
            create_workers(&mut global_queue);

        Self {
            global_queue,
            worker_builders,
            worker_unparkers,
            worker_shutdown_sender: Some(worker_shutdown_sender),
        }
    }
}

fn create_workers<'task>(
    global_queue: &mut Arc<Injector<Task<'task>>>,
) -> (Sender<()>, Vec<WorkerBuilder<'task>>, Vec<Unparker>) {
    let (worker_queues, stealers): (Vec<_>, Vec<_>) = (0..num_cpus::get())
        .map(|_| {
            let worker_queue = Worker::new_fifo();
            let stealer = worker_queue.stealer();
            (worker_queue, stealer)
        })
        .unzip();
    let stealers = Arc::new(stealers);

    let (worker_shutdown_sender, worker_shutdown_receiver) = bounded(0);

    let (workers, unparkers): (Vec<_>, Vec<_>) = worker_queues
        .into_iter()
        .enumerate()
        .map(|(worker_number, worker_queue)| {
            let global_queue = Arc::clone(global_queue);
            let parker = Parker::new();
            let unparker = parker.unparker().clone();

            let worker = WorkerBuilder::new(
                worker_shutdown_receiver.clone(),
                parker,
                worker_queue,
                global_queue,
                Arc::clone(&stealers),
            )
            .with_name(format!("Worker {worker_number}"));

            (worker, unparker)
        })
        .unzip();

    (worker_shutdown_sender, workers, unparkers)
}

impl<'systems> WorkerPool<'systems> {
    #[tracing::instrument(skip(self))]
    fn add_task(&mut self, task: Task<'systems>) {
        self.global_queue.push(task);
        self.notify_all_workers();
    }

    #[tracing::instrument(skip(self))]
    fn add_tasks(&mut self, tasks: Vec<Task<'systems>>) {
        tasks
            .into_iter()
            .for_each(|task| self.global_queue.push(task));
        self.notify_all_workers();
    }

    /// Wakes up all the workers, if they were sleeping.
    #[tracing::instrument(skip(self))]
    fn notify_all_workers(&self) {
        for unparker in &self.worker_unparkers {
            // It's okay that this might be called on an already-awake worker,
            // because that will only cause the worker to do one unnecessary iteration of it's
            // check-for-task and sleep-if-no-tasks-available cycle.
            unparker.unpark();
        }
    }
}

static GLOBAL_POOL: Mutex<Option<WorkerPool>> = Mutex::new(None);

impl<'systems> WorkerPool<'systems> {
    /// Initializes the global worker pool.
    pub fn initialize_global() {
        let mut global_pool = GLOBAL_POOL.lock().expect("Lock should not be poisoned");

        if global_pool.is_some() {
            debug!(
                "not re-initializing global worker pool because it has already been initialized"
            );
            return;
        }

        let pool = WorkerPool::default();
        *global_pool = Some(pool);
        drop(global_pool);

        // Workers need to be scoped, but thread::scope blocks so run it in a separate thread.
        thread::spawn(|| {
            thread::scope(|scope| {
                let mut maybe_global_pool =
                    GLOBAL_POOL.lock().expect("Lock should not be poisoned");
                let global_pool = maybe_global_pool
                    .as_mut()
                    .expect("Global pool should be initialized before calling");
                let (_workers, _worker_panic_guards) = global_pool.start_workers(scope);
                drop(maybe_global_pool);
            });
        });
    }

    /// Places the given tasks into the worker pool task queue, to be executed by workers.
    pub fn dispatch_tasks(tasks: Vec<Task<'static>>) {
        let mut maybe_global_pool = GLOBAL_POOL.lock().expect("Lock should not be poisoned");
        let global_pool = maybe_global_pool
            .as_mut()
            .expect("Global pool should be initialized before calling");
        global_pool.add_tasks(tasks);
    }

    /// Places the given tasks into the worker pool task queue, to be executed by workers.
    pub fn execute_tick<S: Schedule<'static>>(
        schedule: &mut S,
        world: &'static World,
    ) -> ExecutionResult<()> {
        let mut maybe_global_pool = GLOBAL_POOL.lock().expect("Lock should not be poisoned");
        let global_pool = maybe_global_pool
            .as_mut()
            .expect("Global pool should be initialized before calling");
        global_pool.execute_once(schedule, world)
    }

    /// Wakes up all the workers in the global pool, if they were sleeping.
    pub fn globally_notify_all_workers() {
        let mut maybe_global_pool = GLOBAL_POOL.lock().expect("Lock should not be poisoned");
        let global_pool = maybe_global_pool
            .as_mut()
            .expect("Global pool should be initialized before calling");
        global_pool.notify_all_workers();
    }

    /// Starts the given worker threads in the given scope. The [WorkerHandle]s are
    /// returned, along with their panic guards which will be dropped upon worker thread panic.
    pub fn start_workers<'scope>(
        &mut self,
        scope: &'scope Scope<'scope, 'systems>,
    ) -> (Vec<WorkerHandle<'scope>>, Vec<Receiver<()>>) {
        self.worker_builders
            .drain(..)
            .map(|worker| {
                let (panic_guard_sender, panic_guard_receiver) = bounded(0);
                let worker = worker.start(scope, panic_guard_sender);
                (worker, panic_guard_receiver)
            })
            .unzip()
    }
}

impl<'systems> Executor<'systems> for WorkerPool<'systems> {
    #[tracing::instrument(skip_all)]
    fn execute<S: Schedule<'systems>>(
        &mut self,
        mut schedule: S,
        world: &'systems World,
        shutdown_receiver: Receiver<()>,
    ) -> ExecutionResult<()> {
        let worker_shutdown_sender = self
            .worker_shutdown_sender
            .take()
            .ok_or(ExecutionError::AlreadyRunning);

        let execution_result = thread::scope(|scope| {
            let (_workers, worker_panic_guards) = self.start_workers(scope);

            while let Err(TryRecvError::Empty) = shutdown_receiver.try_recv() {
                self.execute_once(&mut schedule, world)?;

                // Check for any dead threads...
                let worker_died = worker_panic_guards
                    .iter()
                    .any(|panic_guard| panic_guard.try_recv() == Err(TryRecvError::Disconnected));
                if worker_died {
                    error!("A worker has died, most likely due to a panic. Shutting down other workers...");
                    break;
                }
            }

            drop(worker_shutdown_sender);
            // Wake up any sleeping workers so they can shut down.
            self.notify_all_workers();
            Ok(())
        });

        info!("Worker pool has exited!");
        execution_result
    }

    #[tracing::instrument(skip_all)]
    fn execute_once<S: Schedule<'systems>>(
        &mut self,
        schedule: &mut S,
        world: &'systems World,
    ) -> ExecutionResult<()> {
        loop {
            debug!("getting currently executable systems...");
            let systems =
                schedule.currently_executable_systems_with_reaction(NewTickReaction::ReturnError);
            match systems {
                Ok(systems) => {
                    debug!("dispatching new tick!");
                    for system in systems {
                        let tasks = create_system_task(system, world);
                        self.add_tasks(tasks);
                    }
                }
                Err(ScheduleError::NewTick) => {
                    debug!("tick is finished!");
                    return Ok(());
                }
                Err(schedule_error) => {
                    return Err(ExecutionError::Schedule(schedule_error));
                }
            }
        }
    }
}

/// Creates tasks based on a given [SystemExecutionGuard]. The execution will be split into
/// multiple tasks if possible, and if heuristics determine it to be suitable.
#[instrument(skip(world))]
#[cfg_attr(feature = "profile", inline(never))]
pub fn create_system_task<'world>(
    system: SystemExecutionGuard<'world>,
    world: &'world World,
) -> Vec<Task<'world>> {
    if let Some(segmentable_system) = system.system.try_as_segment_iterable() {
        // todo(#84): Figure out a smarter segment size heuristic.
        let segment_size = NonZeroU32::new(100).expect("Value is non-zero");
        let segments = segmentable_system.segments(world, segment_size);
        segments
            .into_iter()
            .map(|segment| {
                let cloned_execution_guard = system.finished_sender.clone();
                Task::new(move || {
                    segment.execute();
                    drop(cloned_execution_guard);
                })
            })
            .collect()
    } else if let Some(sequential_system) = system.system.try_as_sequentially_iterable() {
        vec![Task::new(move || {
            sequential_system
                .run(world)
                .expect("A correctly scheduled system will never fail to fetch its parameters");
            drop(system);
        })]
    } else {
        panic!(
            "System {system:?} can neither execute in sequence nor in parallel. \
             All systems should implement one of the two."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam::channel::unbounded;
    use ecs::Unordered;

    #[test]
    fn task_ids_increase() {
        let task0 = Task::default();
        let task1 = Task::default();
        let task2 = Task::default();

        assert!(task0.uid < task1.uid);
        assert!(task1.uid < task2.uid);
    }

    #[test]
    #[should_panic(expected = "Panicking in worker thread!")]
    fn propagates_worker_panic_to_main_thread() {
        let panicking_system = || panic!("Panicking in worker thread!");

        let world = World::default();
        {
            let mut pool = WorkerPool::default();
            pool.add_task(Task::new(panicking_system));

            let (_shutdown_sender, shutdown_receiver) = unbounded();
            pool.execute(Unordered::default(), &world, shutdown_receiver)
                .unwrap();
        }
    }
}
