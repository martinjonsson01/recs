//! Different ways to iterate over all queried entities in a [`System`].

use super::*;
use crate::World;

/// Execution of a single [`System`] in a sequential order.
pub trait SequentiallyIterable {
    /// Executes the system on each entity matching its query.
    ///
    /// Systems that do not query anything run once per tick.
    fn run(&self, world: &World) -> SystemResult<()>;
}

impl<Function> SequentiallyIterable for FunctionSystem<Function, ()>
where
    Function: Fn() + Send + Sync + 'static,
{
    fn run(&self, _world: &World) -> SystemResult<()> {
        (self.function)();
        Ok(())
    }
}

macro_rules! impl_sequentially_iterable_system {
    ($($parameter:expr),*) => {
        paste! {
            impl<Function, $([<P$parameter>]: SystemParameter,)*> SequentiallyIterable
                for FunctionSystem<Function, ($([<P$parameter>],)*)>
            where
                Function: Fn($([<P$parameter>],)*) + Send + Sync + 'static,
            {

                fn run(&self, world: &World) -> SystemResult<()> {
                    let signature = vec![$(<[<P$parameter>] as SystemParameter>::signature(),)*];

                    $(let mut [<borrowed_$parameter>] = <[<P$parameter>] as SystemParameter>::borrow(world, &signature).map_err(SystemError::MissingParameter)?;)*

                    // SAFETY: This is safe because the result from fetch_parameter will not outlive borrowed
                    unsafe {
                        while let ($(Some([<parameter_$parameter>]),)*) = (
                            $(<[<P$parameter>] as SystemParameter>::fetch_parameter(&mut [<borrowed_$parameter>]),)*
                        ) {
                            if let ($(Some([<parameter_$parameter>]),)*) = (
                                $([<parameter_$parameter>],)*
                            ) {
                                (self.function)($([<parameter_$parameter>],)*);
                            }
                        }
                    }
                    Ok(())
                }
            }
        }
    }
}

impl_sequentially_iterable_system!(0);
impl_sequentially_iterable_system!(0, 1);
impl_sequentially_iterable_system!(0, 1, 2);
impl_sequentially_iterable_system!(0, 1, 2, 3);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7, 8);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14);
impl_sequentially_iterable_system!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
