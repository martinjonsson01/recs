use std::iter;

use crossbeam_deque::{Injector, Stealer, Worker};

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
struct Task<Function, Data> {
    function: Function,
    data: Data,
}

impl<Function, Data> Task<Function, Data>
where
    Function: Fn(Data),
{
    #[allow(unused)]
    fn run(self) {
        (self.function)(self.data);
    }
}

#[derive(Debug, Default)]
pub struct ThreadPool<Function, Data> {
    _injector: Injector<Task<Function, Data>>,
    _workers: Vec<WorkerThread<Function, Data>>,
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
struct WorkerThread<Function, Data> {
    local_queue: Worker<Task<Function, Data>>,
    global_queue: Injector<Task<Function, Data>>,
    stealers: Vec<Stealer<Task<Function, Data>>>,
}

impl<Function, Data> WorkerThread<Function, Data>
where
    Function: Fn(Data),
{
    #[allow(unused)]
    fn new(
        global_queue: Injector<Task<Function, Data>>,
        stealers: Vec<Stealer<Task<Function, Data>>>,
    ) -> Self {
        Self {
            local_queue: Worker::new_fifo(),
            global_queue,
            stealers,
        }
    }

    #[allow(unused)]
    fn run(&self) {
        if let Some(task) = self.find_task() {
            task.run();
        }
    }
}

impl<Function, Data> Peasant for WorkerThread<Function, Data> {
    type Task = Task<Function, Data>;

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
    use std::cell::RefCell;
    use std::rc::Rc;

    use rand::Rng;

    use super::*;

    type MockTask = Option<Task<fn(i32), i32>>;

    // Generates a unique mock task.
    fn mock_task() -> MockTask {
        let mut rng = rand::thread_rng();
        Some(Task {
            function: |_| {},
            data: rng.gen(),
        })
    }

    struct MockedSystem {
        worker: WorkerThread<fn(i32), i32>,
        workers_task: MockTask,
        others_task: MockTask,
        globals_task: MockTask,
    }

    /// Sets up a simple scenario:
    /// there are two workers: worker       and other
    /// with tasks:            workers_task and others_task
    /// and a task in global queue: globals_task
    fn mock_workers(
        workers_task: MockTask,
        others_task: MockTask,
        globals_task: MockTask,
    ) -> MockedSystem {
        let global_queue = Injector::new();
        if let Some(globals_task) = globals_task {
            global_queue.push(globals_task)
        }
        let other_worker = Worker::new_fifo();
        if let Some(others_task) = others_task {
            other_worker.push(others_task)
        }
        let stealers = vec![other_worker.stealer()];
        let worker = WorkerThread::new(global_queue, stealers);
        if let Some(workers_task) = workers_task {
            worker.local_queue.push(workers_task)
        }
        MockedSystem {
            worker,
            workers_task,
            others_task,
            globals_task,
        }
    }

    #[test]
    fn worker_takes_task_from_local_queue_first() {
        let MockedSystem {
            worker,
            workers_task,
            ..
        } = mock_workers(mock_task(), mock_task(), mock_task());

        let task = worker.find_task().unwrap();

        assert_eq!(workers_task.unwrap(), task);
    }

    #[test]
    fn worker_takes_global_task_when_local_queue_empty() {
        let MockedSystem {
            worker,
            globals_task,
            ..
        } = mock_workers(None, mock_task(), mock_task());

        let task = worker.find_task().unwrap();

        assert_eq!(globals_task.unwrap(), task);
    }

    #[test]
    fn worker_steals_task_when_local_and_global_queue_empty() {
        let MockedSystem {
            worker,
            others_task,
            ..
        } = mock_workers(None, mock_task(), None);

        let task = worker.find_task().unwrap();

        assert_eq!(others_task.unwrap(), task);
    }

    #[test]
    fn worker_runs_given_task() {
        let worker = WorkerThread::new(Injector::default(), vec![]);
        let has_task_run = Rc::new(RefCell::new(false));
        let has_task_run_clone = Rc::clone(&has_task_run);
        let function = move |value: i32| {
            println!("{value}");
            *has_task_run_clone.borrow_mut() = true
        };
        let task = Task { function, data: 12 };
        worker.local_queue.push(task);

        worker.run();

        let has_run_task = has_task_run.borrow();
        assert!(*has_run_task)
    }
}
