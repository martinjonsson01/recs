//! Different ways to iterate over all queried entities in a [`System`].

use super::*;
use crate::{intersection_of_multiple_sets, World};
use itertools::Itertools;
use std::num::NonZeroU32;

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

/// Execution of a single [`System`] in a segmented manner, meaning different segments are
/// safe to execute at different times (i.e. concurrently).
pub trait SegmentIterable {
    /// Divides the iteration up into segments with target size `segment_size`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::num::NonZeroU32;
    /// # use ecs::systems::{IntoSystem, Read, System, SystemError};
    /// # use ecs::systems::iteration::SystemSegment;
    /// # use ecs::World;
    /// # let system = (|_: Read<i32>| ()).into_system();
    /// # let segment_iterable = system.try_as_segment_iterable().unwrap();
    /// # let world = World::default();
    ///
    /// let segment_size = NonZeroU32::new(10).expect("Value is non-zero.");
    /// let segments: Vec<SystemSegment> = segment_iterable.segments(&world, segment_size)?;
    ///
    /// for segment in segments {
    ///     segment.execute();
    /// }
    ///
    /// # Ok::<(), SystemError>(())
    /// ```
    fn segments(&self, world: &World, segment_size: NonZeroU32)
        -> SystemResult<Vec<SystemSegment>>;
}

impl<Function> SegmentIterable for FunctionSystem<Function, ()>
where
    Function: Fn() + Send + Sync + 'static,
{
    fn segments(
        &self,
        _world: &World,
        _segment_size: NonZeroU32,
    ) -> SystemResult<Vec<SystemSegment>> {
        let function = Arc::clone(&self.function);
        let execution = move || {
            function();
        };
        let segment = SystemSegment {
            system_name: self.function_name.to_owned(),
            executable: Box::new(execution),
        };
        Ok(vec![segment])
    }
}

/// A part of the full execution of a single [`FunctionSystem`].
pub struct SystemSegment {
    /// The name of the [`System`] this is a segment of.
    pub system_name: String,
    executable: Box<dyn FnOnce()>,
}

impl Debug for SystemSegment {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemSegment")
            .field("system_name", &self.system_name)
            .finish()
    }
}

impl SystemSegment {
    /// Runs part of the original [`System`]'s iterations.
    pub fn execute(self) {
        (self.executable)()
    }
}

macro_rules! impl_segment_iterable_system {
    ($($parameter:expr),*) => {
        paste! {
            impl<Function, $([<P$parameter>]: SystemParameter + 'static,)*> SegmentIterable
                for FunctionSystem<Function, ($([<P$parameter>],)*)>
            where
                Function: Fn($([<P$parameter>],)*) + Send + Sync + 'static,
            {

                fn segments(
                    &self,
                    world: &World,
                    segment_size: NonZeroU32,
                ) -> SystemResult<Vec<SystemSegment>> {
                    let query: Query<($([<P$parameter>],)*)> = Query {
                        phantom: Default::default(),
                        world,
                    };

                    let segments = query
                        .into_iter()
                        .chunks(segment_size.get() as usize)
                        .into_iter()
                        .map(|chunk| chunk.collect())
                        .map(|segment: Vec<_>| {
                            let function = Arc::clone(&self.function);
                            let execution = move || {
                                for ($([<parameter$parameter>],)*) in segment {
                                    function($([<parameter$parameter>],)*);
                                }
                            };
                            SystemSegment {
                                system_name: self.function_name.to_owned(),
                                executable: Box::new(execution),
                            }
                        })
                        .collect();

                    Ok(segments)
                }
            }
        }
    }
}

invoke_for_each_parameter_count!(impl_segment_iterable_system);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Application;
    use proptest::prop_compose;
    use test_strategy::proptest;

    #[derive(Debug)]
    struct MockParameter;

    prop_compose! {
        fn arb_function_system()
                              ((segment_size, expected_segment_count) in (1..10u32, 1..10u32))
                              -> (u32, u32, Box<dyn System>, Application) {
            let system = |_: Read<MockParameter>| ();
            #[allow(trivial_casts)] // Compiler won't coerce `FunctionSystem` to `dyn System` for some reason.
            let boxed_system = Box::new(system.into_system()) as Box<dyn System>;

            let mut app = Application::default();
            let entity_count = segment_size * expected_segment_count;
            for _ in 0..entity_count {
                let entity = app.create_entity().unwrap();
                app.add_component(entity, MockParameter).unwrap();
            }

            (segment_size, expected_segment_count, boxed_system, app)
        }
    }

    #[proptest]
    fn perfectly_sized_iterations_are_divided_into_equal_batches(
        #[strategy(arb_function_system())] input: (u32, u32, Box<dyn System>, Application),
    ) {
        let (segment_size, expected_segment_count, system, app) = input;
        let segment_size = NonZeroU32::new(segment_size).unwrap();

        let segment_iterable = system.try_as_segment_iterable().unwrap();

        let segments = segment_iterable.segments(&app.world, segment_size).unwrap();

        assert_eq!(expected_segment_count as usize, segments.len())
    }
}
