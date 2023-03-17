//! Executors that can run systems according to a given `ecs::Schedule`
//! in different ways, such as on a thread pool.

use crossbeam::channel::Receiver;
use crossbeam::deque::Injector;
use crossbeam::sync::Unparker;
use ecs::{Executor, Schedule, World};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::instrument;

mod worker;

/// A single unit of work that can be executed once and is then completed.
struct Task<'a> {
    uid: usize,
    // Might be a good optimization to have RecurringTask that uses an FnMut later on?
    _function: Box<dyn FnOnce() + Send + 'a>,
}

impl<'a> std::fmt::Debug for Task<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Task<{}>", self.uid)
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
            _function: Box::new(function),
        }
    }

    #[instrument(skip(self))]
    fn run(self) {
        (self._function)()
    }
}

/// Executes tasks on a set of workers running on separate threads.
#[derive(Debug, Default)]
pub struct WorkerPool<'task> {
    /// The main work queue that workers will take tasks from.
    _global_queue: Arc<Injector<Task<'task>>>,
    /// Handles to workers that can be used to wake them up, in case they were sleeping due to
    /// lack of tasks.
    _worker_unparkers: Vec<Unparker>,
}

impl<'task> WorkerPool<'task> {
    #[tracing::instrument]
    fn add_task(&mut self, task: Task<'task>) {
        self._global_queue.push(task);
        self.notify_all_workers();
    }

    /// Wakes up all the workers, if they were sleeping.
    #[tracing::instrument]
    fn notify_all_workers(&self) {
        for unparker in &self._worker_unparkers {
            // todo: what will happen if this is called on a thread not sleeping?
            // todo: will it not go to sleep the next time it should?
            unparker.unpark();
        }
    }
}

impl<'task> Executor for WorkerPool<'task> {
    #[tracing::instrument]
    fn execute<'a, S: Schedule<'a>>(
        &mut self,
        _schedule: S,
        _world: &World,
        _shutdown_receiver: Receiver<()>,
    ) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn task_ids_increase() {
        let task0 = Task::default();
        let task1 = Task::default();
        let task2 = Task::default();

        assert!(task0.uid < task1.uid);
        assert!(task1.uid < task2.uid);
    }
}
