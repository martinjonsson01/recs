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
                    let query: Query<($([<P$parameter>],)*)> = Query {
                        phantom: PhantomData::default(),
                        world
                    };

                    let query_iterator = query.try_into_iter().map_err(SystemError::MissingParameter)?;
                    for ($([<parameter_$parameter>],)*) in query_iterator {
                        (self.function)($([<parameter_$parameter>],)*);
                    }

                    Ok(())
                }
            }
        }
    }
}

invoke_for_each_parameter_count!(impl_sequentially_iterable_system);
