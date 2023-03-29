//! Different ways to iterate over all queried entities in a [`System`].

use super::*;
use crate::{intersection_of_multiple_sets, World};

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
                    let base_signature: Vec<TypeId> = [$([<P$parameter>]::base_signature(),)*]
                        .into_iter()
                        .flatten()
                        .collect();

                    let universe = world.get_archetype_indices(&base_signature);

                    let archetypes_indices: Vec<_> = intersection_of_multiple_sets(&[
                        universe.clone(),
                        $([<P$parameter>]::filter(&universe, world),)*
                    ])
                    .into_iter()
                    .collect();

                    $(let mut [<borrowed_$parameter>] = [<P$parameter>]::borrow(world, &archetypes_indices).map_err(SystemError::MissingParameter)?;)*

                    // SAFETY: This is safe because the result from fetch_parameter will not outlive borrowed
                    unsafe {
                        if $([<P$parameter>]::iterates_over_entities() ||)* false {
                            while let ($(Some([<parameter_$parameter>]),)*) = (
                                $([<P$parameter>]::fetch_parameter(&mut [<borrowed_$parameter>]),)*
                            ) {
                                if let ($(Some([<parameter_$parameter>]),)*) = (
                                    $([<parameter_$parameter>],)*
                                ) {
                                    (self.function)($([<parameter_$parameter>],)*);
                                }
                            }
                        } else if let ($(Some(Some([<parameter_$parameter>])),)*) = (
                                $([<P$parameter>]::fetch_parameter(&mut [<borrowed_$parameter>]),)*
                        ) {
                            (self.function)($([<parameter_$parameter>],)*);
                        }
                    }
                    Ok(())
                }
            }
        }
    }
}

invoke_for_each_parameter_count!(impl_sequentially_iterable_system);
