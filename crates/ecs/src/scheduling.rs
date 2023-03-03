use crate::{System, World};
use crossbeam::channel::{Receiver, TryRecvError};
use std::fmt::Debug;

/// A way of scheduling the order in which systems execute.
pub trait Schedule<'systems> {
    /// Generates a schedule for the given systems.
    fn generate(systems: &'systems [Box<dyn System>]) -> Self;

    /// Gets the next batch of systems to execute.
    ///
    /// These should be safe to run concurrently.
    fn next_batch(&mut self) -> Vec<&'systems dyn System>;
}

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
pub struct Linear<'a> {
    systems: &'a [Box<dyn System>],
    current_system: usize,
}
impl<'a> Schedule<'a> for Linear<'a> {
    fn generate(systems: &'a [Box<dyn System>]) -> Self {
        Self {
            systems,
            current_system: 0,
        }
    }

    fn next_batch(&mut self) -> Vec<&'a dyn System> {
        let batch = vec![self.systems[self.current_system].as_ref()];
        self.current_system = (self.current_system + 1) % self.systems.len();
        batch
    }
}
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
pub struct Unordered<'a> {
    systems: &'a [Box<dyn System>],
}
impl<'a> Schedule<'a> for Unordered<'a> {
    fn generate(systems: &'a [Box<dyn System>]) -> Self {
        Self { systems }
    }

    fn next_batch(&mut self) -> Vec<&'a dyn System> {
        self.systems.iter().map(|s| s.as_ref()).collect()
    }
}

/// A way of executing systems according to a schedule.
pub trait ScheduleExecutor<'a>: Debug {
    /// Executes systems in a world according to a given schedule.
    fn execute<S: Schedule<'a>>(
        &mut self,
        schedule: S,
        world: &'a World,
        shutdown_receiver: Receiver<()>,
    );
}

#[derive(Debug, Default)]
pub struct Sequential;

impl<'a> ScheduleExecutor<'a> for Sequential {
    fn execute<S: Schedule<'a>>(
        &mut self,
        mut schedule: S,
        world: &'a World,
        shutdown_receiver: Receiver<()>,
    ) {
        while let Err(TryRecvError::Empty) = shutdown_receiver.try_recv() {
            for system in schedule.next_batch() {
                system.run(world);
            }
        }
    }
}
