use std::time::Duration;
use std::{iter, thread};

use crossbeam::deque::{Injector, Stealer, Worker};

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

impl<Function: Send + 'static, Data: Send + 'static> WorkerThread<Function, Data>
where
    Function: Fn(Data),
{
    #[allow(unused)]
    fn start(
        global_queue: Injector<Task<Function, Data>>,
        stealers: Vec<Stealer<Task<Function, Data>>>,
    ) -> thread::JoinHandle<()> {
        Self::start_with_tasks(global_queue, stealers, vec![])
    }

    fn start_with_tasks(
        global_queue: Injector<Task<Function, Data>>,
        stealers: Vec<Stealer<Task<Function, Data>>>,
        tasks: impl IntoIterator<Item = Task<Function, Data>> + Send + 'static,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let worker = Self {
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

    #[allow(unused)]
    fn run(&self) {
        loop {
            if let Some(task) = self.find_task() {
                task.run();
            }
            thread::sleep(Duration::from_millis(10));
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
    use std::sync::Arc;
    use std::thread;

    use crossbeam::atomic::AtomicCell;
    use crossbeam::sync::Parker;
    use itertools::Itertools;
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

    fn mock_worker<Function, Data>(
        global_queue: Injector<Task<Function, Data>>,
        stealers: Vec<Stealer<Task<Function, Data>>>,
    ) -> WorkerThread<Function, Data> {
        WorkerThread {
            local_queue: Worker::new_fifo(),
            global_queue,
            stealers,
        }
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
        let worker = mock_worker(global_queue, stealers);
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
        let parker = Parker::new();
        let unparker = parker.unparker().clone();

        let has_task_run = Arc::new(AtomicCell::new(false));
        let has_task_run_clone = Arc::clone(&has_task_run);
        let function = move |value: i32| {
            println!("{value}");
            has_task_run_clone.store(true);
            unparker.unpark();
        };
        let task = Task { function, data: 12 };

        WorkerThread::start_with_tasks(Injector::default(), vec![], vec![task]);
        // Wait for task to complete
        parker.park();

        assert!(has_task_run.take())
    }

    #[test]
    fn worker_runs_on_another_thread() {
        let parker = Parker::new();
        let unparker = parker.unparker().clone();

        let worker_thread_id = Arc::new(AtomicCell::new(None));
        let worker_thread_id_clone = Arc::clone(&worker_thread_id);
        let function = move |value: i32| {
            println!("{value}");
            worker_thread_id_clone.store(Some(thread::current().id()));
            unparker.unpark();
        };
        let task = Task { function, data: 12 };

        WorkerThread::start_with_tasks(Injector::default(), vec![], vec![task]);
        // Wait for task to complete
        parker.park();

        let main_thread_id = thread::current().id();
        assert_ne!(
            Some(main_thread_id),
            worker_thread_id.take(),
            "worker shouldn't run on main thread"
        )
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

            let function = move |value: i32| {
                println!("{value}");
                has_task_run_clone.store(true);
                // Last task to run unparks
                if value == task_count - 1 {
                    unparker_clone.unpark();
                }
            };

            let task = Task { function, data: i };
            tasks.push(task);
            have_tasks_run.push(has_task_run);
        }

        WorkerThread::start_with_tasks(Injector::default(), vec![], tasks);
        // Wait for last task to complete
        parker.park();

        let all_tasks_ran = vec![true].repeat(task_count as usize);
        let have_tasks_run: Vec<_> = have_tasks_run
            .iter()
            .map(|has_run| has_run.take())
            .collect();
        assert_eq!(all_tasks_ran, have_tasks_run, "all tasks should run")
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

            let function = move |value: i32| {
                let id = thread::current().id();
                println!("thread {id:?}: {value}");
                worker_thread_id_clone.store(Some(id));
                // Last task to run unparks
                if value == task_count - 1 {
                    unparker_clone.unpark();
                }
            };
            let task = Task { function, data: i };
            tasks.push(task);
            thread_ids.push(Arc::clone(&worker_thread_id));
        }

        WorkerThread::start_with_tasks(Injector::default(), vec![], tasks);
        // Wait for last task to complete
        parker.park();

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
}
