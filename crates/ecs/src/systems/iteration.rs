//! Different ways to iterate over all queried entities in a [`System`].

use super::*;
use crate::World;
use itertools::izip;

/// Execution of a single [`System`] in a sequential order.
pub trait SequentiallyIterable: Send + Sync {
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
                    let archetypes = <($([<P$parameter>],)*) as SystemParameters>::get_archetype_indices(world);

                    $(let mut [<borrowed_$parameter>] = [<P$parameter>]::borrow(world, &archetypes).map_err(SystemError::MissingParameter)?;)*
                    let mut segments = ($([<P$parameter>]::split_borrowed_data(&mut [<borrowed_$parameter>], Segment::Single),)*);

                    let query: Query<($([<P$parameter>],)*)> = Query {
                        phantom: PhantomData::default(),
                        borrowed: unsafe { std::mem::transmute(&mut segments) }, // query is dropped before segments
                        world,
                        archetypes,
                        iterate_over_entities: $([<P$parameter>]::iterates_over_entities())||*,
                        iterated_once: false,
                    };

                    for ($([<parameter_$parameter>],)*) in query {
                        (self.function)($([<parameter_$parameter>],)*);
                    }

                    Ok(())
                }
            }
        }
    }
}

invoke_for_each_parameter_count!(impl_sequentially_iterable_system);

/// Execution of a single [`System`] in a parallel.
pub trait ParallelIterable: Send + Sync {
    /// Executes the system on each entity matching its query.
    ///
    /// Systems that do not query anything run once per tick.
    fn run(&self, world: &World, segment: Segment) -> SystemResult<()>;
}

impl<Function> ParallelIterable for FunctionSystem<Function, ()>
where
    Function: Fn() + Send + Sync + 'static,
{
    fn run(&self, _world: &World, _segment: Segment) -> SystemResult<()> {
        (self.function)();
        Ok(())
    }
}

macro_rules! impl_parallel_iterable_system {
    ($($parameter:expr),*) => {
        paste! {
            impl<Function, $([<P$parameter>]: SystemParameter,)*> ParallelIterable
                for FunctionSystem<Function, ($([<P$parameter>],)*)>
            where
                Function: Fn($([<P$parameter>],)*) + Send + Sync + 'static,
            {
                fn run(&self, world: &World, segment: Segment) -> SystemResult<()> {
                    let archetypes = <($([<P$parameter>],)*) as SystemParameters>::get_archetype_indices(world);

                    $(let mut [<borrowed_$parameter>] = [<P$parameter>]::borrow(world, &archetypes).map_err(SystemError::MissingParameter)?;)*
                    $(let mut [<segments_$parameter>] = [<P$parameter>]::split_borrowed_data(&mut [<borrowed_$parameter>], segment);)*

                    // SAFETY: This is safe because the result from fetch_parameter will not outlive borrowed
                    unsafe {
                        if $([<P$parameter>]::iterates_over_entities() )||* {
                            rayon::scope(|s| {
                                #[allow(unused_parens)]
                                for ($(mut [<segment_$parameter>]),*) in izip!($([<segments_$parameter>],)*) {
                                    s.spawn(move |_| {
                                        while let ($(Some([<parameter_$parameter>]),)*) = (
                                            $([<P$parameter>]::fetch_parameter(&mut [<segment_$parameter>]),)*
                                        ) {
                                            if let ($(Some([<parameter_$parameter>]),)*) = (
                                                $([<parameter_$parameter>],)*
                                            ) {
                                                (self.function)($([<parameter_$parameter>],)*);
                                            }
                                        }
                                    });
                                }
                            });

                        } else if let ($(Some(Some([<parameter_$parameter>])),)*) = (
                            $([<P$parameter>]::fetch_parameter([<segments_$parameter>].get_mut(0).expect("there should always be at least one segment")),)*
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

invoke_for_each_parameter_count!(impl_parallel_iterable_system);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Application, BasicApplication};
    use std::sync::{Arc, Mutex};
    use test_utils::{A, B};

    #[derive(Debug)]
    struct MockParameter;

    fn set_up_system_that_records_iterated_components() -> (Arc<Mutex<Vec<A>>>, Box<dyn System>) {
        let segment_iterated_components = Arc::new(Mutex::new(vec![]));
        let segment_iterated_components_ref = Arc::clone(&segment_iterated_components);
        let segment_function = move |a: Read<A>| {
            segment_iterated_components_ref.lock().unwrap().push(*a);
        };
        let system = segment_function.into_system();
        (segment_iterated_components, Box::new(system))
    }

    #[test]
    fn segmented_iteration_traverses_same_component_values_as_sequential_iteration() {
        let expected_components = HashSet::from([A(1431), A(123), A(94), A(2)]);

        let mut application = BasicApplication::default();

        for expected_component in expected_components.clone() {
            let entity = application.create_entity().unwrap();
            application
                .add_component(entity, expected_component)
                .unwrap();
        }

        let (sequentially_iterated_components, system) =
            set_up_system_that_records_iterated_components();
        let sequential_iterable = system.try_as_sequentially_iterable().unwrap();
        sequential_iterable.run(&application.world).unwrap();

        let (segment_iterated_components, system) =
            set_up_system_that_records_iterated_components();
        let segmented_iterable = system.try_as_parallel_iterable().unwrap();
        let segment_size = Segment::Size(2);
        segmented_iterable
            .run(&application.world, segment_size)
            .unwrap();

        let sequential_components_guard = sequentially_iterated_components.lock().unwrap();
        let sequential_components = sequential_components_guard
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let segment_components_guard = segment_iterated_components.lock().unwrap();
        let segment_components = segment_components_guard
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        assert_eq!(
            expected_components, segment_components,
            "should have iterated expected components"
        );
        assert_eq!(
            segment_components, sequential_components,
            "should have iterated same as sequential"
        )
    }

    #[test]
    fn system_cannot_be_segment_iterated_if_a_parameter_does_not_support_parallelization() {
        #[derive(Debug)]
        struct NonParallelParameter;

        impl SystemParameter for NonParallelParameter {
            type BorrowedData<'components> = ();
            type SegmentData<'components> = ();

            fn borrow<'world>(
                _world: &'world World,
                _archetypes: &[ArchetypeIndex],
            ) -> SystemParameterResult<Self::BorrowedData<'world>> {
                unimplemented!()
            }

            fn split_borrowed_data<'borrowed>(
                _: &'borrowed mut Self::BorrowedData<'_>,
                _: Segment,
            ) -> Vec<Self::SegmentData<'borrowed>> {
                unimplemented!()
            }

            unsafe fn fetch_parameter(
                _borrowed: &mut Self::SegmentData<'_>,
            ) -> Option<Option<Self>> {
                unimplemented!()
            }

            fn component_accesses() -> Vec<ComponentAccessDescriptor> {
                unimplemented!()
            }

            fn iterates_over_entities() -> bool {
                unimplemented!()
            }

            fn base_signature() -> Option<TypeId> {
                unimplemented!()
            }

            fn support_parallelization() -> bool {
                false
            }
        }

        let system_function = |_: Read<A>, _: Read<B>, _: NonParallelParameter| ();
        let system = system_function.into_system();

        let result = system.try_as_parallel_iterable();

        assert!(result.is_none());
    }
}
