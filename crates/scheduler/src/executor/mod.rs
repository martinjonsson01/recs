//! Executors that can run systems according to a given `ecs::Schedule`
//! in different ways, such as on a thread pool.

pub use crate::executor::worker::{WorkerBuilder, WorkerHandle};
use crossbeam::channel::{bounded, Receiver, Sender, TryRecvError};
use crossbeam::deque::{Injector, Worker};
use crossbeam::sync::{Parker, Unparker};
use ecs::systems::SystemError;
use ecs::{
    ExecutionError, ExecutionResult, Executor, NewTickReaction, Schedule, ScheduleError,
    SystemExecutionGuard, World,
};
use std::mem;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, instrument};

mod worker;

/// A single unit of work that can be executed once and is then completed.
pub struct Task {
    uid: usize,
    // Might be a good optimization to have RecurringTask that uses an FnMut later on?
    function: Box<dyn FnOnce() + Send>,
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task").field("uid", &self.uid).finish()
    }
}

impl Default for Task {
    fn default() -> Self {
        Task::new(|| {})
    }
}

impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}

impl Task {
    #[instrument(skip_all)]
    fn new(function: impl FnOnce() + Send + 'static) -> Self {
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
pub struct WorkerPool {
    /// The main work queue that workers will take tasks from.
    global_queue: Arc<Injector<Task>>,
    /// The workers of the pool.
    workers: Vec<WorkerHandle>,
    /// Panic guards can be used to detect when a worker has panicked.
    ///
    /// When the channel is closed, the worker has shut down (could be due to a panic if not planned).
    worker_panic_guards: Vec<Receiver<()>>,
    /// Handles to workers that can be used to wake them up, in case they were sleeping due to
    /// lack of tasks.
    worker_unparkers: Vec<Unparker>,
    /// Can be dropped to shut down all workers.
    worker_shutdown_sender: Option<Sender<()>>,
}

/// All worker threads are joined when [`WorkerPool`] is dropped.
impl Drop for WorkerPool {
    fn drop(&mut self) {
        let worker_shutdown_sender = self
            .worker_shutdown_sender
            .take()
            .ok_or(ExecutionError::AlreadyRunning);
        drop(worker_shutdown_sender);
        // Wake up any sleeping workers so they can shut down.
        self.notify_all_workers();

        // Drop worker handles in order to join them one by one.
        drop(self.workers.drain(..));

        info!("Worker pool has exited!");
    }
}

impl Default for WorkerPool {
    fn default() -> Self {
        let mut global_queue = Arc::new(Injector::new());
        let (worker_shutdown_sender, mut worker_builders, worker_unparkers) =
            create_workers(&mut global_queue);

        let (workers, worker_panic_guards) = worker_builders
            .drain(..)
            .map(|worker| {
                let (panic_guard_sender, panic_guard_receiver) = bounded(0);
                let worker = worker.start(panic_guard_sender);
                (worker, panic_guard_receiver)
            })
            .unzip();

        Self {
            global_queue,
            workers,
            worker_panic_guards,
            worker_unparkers,
            worker_shutdown_sender: Some(worker_shutdown_sender),
        }
    }
}

fn create_workers(
    global_queue: &mut Arc<Injector<Task>>,
) -> (Sender<()>, Vec<WorkerBuilder>, Vec<Unparker>) {
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

impl WorkerPool {
    #[tracing::instrument(skip(self))]
    fn add_task(&mut self, task: Task) {
        self.global_queue.push(task);
        self.notify_all_workers();
    }

    #[tracing::instrument(skip(self))]
    fn add_tasks(&mut self, tasks: Vec<Task>) {
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

impl Executor for WorkerPool {
    #[tracing::instrument(skip_all)]
    fn execute_once<S: Schedule>(
        &mut self,
        schedule: &mut S,
        world: &World,
    ) -> ExecutionResult<()> {
        loop {
            debug!("getting currently executable systems...");
            let systems =
                schedule.currently_executable_systems_with_reaction(NewTickReaction::ReturnError);
            match systems {
                Ok(systems) => {
                    debug!("dispatching new tick!");
                    for system in systems {
                        // SAFETY: The tasks will finish executing before the `world` reference is
                        // dropped, because of the loop in this function not exiting until
                        // an entire tick is finished. An entire tick is finished when all systems
                        // (i.e. tasks) have finished executing.
                        let tasks = unsafe { create_system_task(system, world) };
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

            // Check for any dead threads...
            let worker_died = self
                .worker_panic_guards
                .iter()
                .any(|panic_guard| panic_guard.try_recv() == Err(TryRecvError::Disconnected));
            if worker_died {
                error!(
                    "A worker has died, most likely due to a panic. Shutting down other workers..."
                );
                return Err(ExecutionError::System(SystemError::Panic));
            }
        }
    }
}

/// Creates tasks based on a given [SystemExecutionGuard]. The execution will be split into
/// multiple tasks if possible, and if heuristics determine it to be suitable.
///
/// # Safety
/// The `world` reference argument will be stored in the returned `Task`s without a lifetime
/// annotation. This means that `world` cannot be dropped until all of the `Task`s are dropped.
#[instrument(skip(world))]
#[cfg_attr(feature = "profile", inline(never))]
unsafe fn create_system_task(system_guard: SystemExecutionGuard, world: &World) -> Vec<Task> {
    if let Some(segmentable_system) = system_guard.system.try_as_segment_iterable() {
        // todo(#84): Figure out a smarter segment size heuristic.
        let segment_size = NonZeroU32::new(100).expect("Value is non-zero");
        let segments = segmentable_system.segments(world, segment_size);
        segments
            .into_iter()
            .map(|segment| {
                let cloned_execution_guard = system_guard.finished_sender.clone();
                Task::new(move || {
                    segment.execute();
                    drop(cloned_execution_guard);
                })
            })
            .collect()
    } else if let Some(sequential_system) = system_guard.system.try_as_sequentially_iterable() {
        let world = mem::transmute(world);
        vec![Task::new(move || {
            sequential_system
                .run(world)
                .expect("A correctly scheduled system will never fail to fetch its parameters");
            drop(system_guard.finished_sender);
        })]
    } else {
        panic!(
            "System {system_guard:?} can neither execute in sequence nor in parallel. \
             All systems should implement one of the two."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let mut pool = WorkerPool::default();
        pool.add_task(Task::new(panicking_system));
        // Dropping the pool joins all threads, so any panic will be propagated.
    }
}
