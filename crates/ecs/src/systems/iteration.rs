//! Different ways to iterate over all queried entities in a [`System`].

use super::*;
use crate::World;
use itertools::Itertools;
use std::num::NonZeroU32;

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
            impl<Function, $([<P$parameter>]: SystemParameter + 'static,)*> SequentiallyIterable
                for FunctionSystem<Function, ($([<P$parameter>],)*)>
            where
                Function: Fn($([<P$parameter>],)*) + Send + Sync + 'static,
            {

                fn run(&self, world: &World) -> SystemResult<()> {
                    let query: Query<($([<P$parameter>],)*)> = Query::new(world, self);

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

/// Execution of a single [`System`] in a segmented manner, meaning different segments are
/// safe to execute at different times (i.e. concurrently).
pub trait SegmentIterable: Debug {
    /// Divides the iteration up into segments with target size `segment_size`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::num::NonZeroU32;
    /// # use std::sync::Arc;
    /// # use ecs::systems::{IntoSystem, Read, System, SystemError};
    /// # use ecs::systems::iteration::SystemSegment;
    /// # use ecs::World;
    /// # let system = (|_: Read<i32>| ()).into_system();
    /// # let segment_iterable = system.try_as_segment_iterable().unwrap();
    /// # let world = Arc::new(World::default());
    ///
    /// let segment_size = NonZeroU32::new(10).expect("Value is non-zero.");
    /// let segments: Vec<SystemSegment> = segment_iterable.segments(&world, segment_size);
    ///
    /// for segment in segments {
    ///     segment.execute();
    /// }
    /// ```
    fn segments(&self, world: &World, segment_size: NonZeroU32) -> Vec<SystemSegment>;
}

impl<Function> SegmentIterable for FunctionSystem<Function, ()>
where
    Function: Fn() + Send + Sync + 'static,
{
    fn segments(&self, _world: &World, _segment_size: NonZeroU32) -> Vec<SystemSegment> {
        let function = Arc::clone(&self.function);
        let execution = move || {
            function();
        };
        let segment = SystemSegment {
            system_name: self.function_name.to_owned(),
            executable: Box::new(execution),
        };
        vec![segment]
    }
}

/// A part of the full execution of a single [`FunctionSystem`].
pub struct SystemSegment {
    /// The name of the [`System`] this is a segment of.
    pub system_name: String,
    executable: Box<dyn FnOnce() + Send + Sync>,
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
                ) -> Vec<SystemSegment> {
                    let query: Query<($([<P$parameter>],)*)> = Query {
                        phantom: Default::default(),
                        world,
                        system: self,
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

                    segments
                }
            }
        }
    }
}

invoke_for_each_parameter_count!(impl_segment_iterable_system);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Application, BasicApplication};
    use proptest::prop_compose;
    use std::sync::Mutex;
    use test_strategy::proptest;
    use test_utils::{A, B};
    use test_utils::{D, E, F};

    #[derive(Debug)]
    struct MockParameter;

    prop_compose! {
        fn arb_function_system()
                              ((segment_size, expected_segment_count) in (1..10u32, 1..10u32))
                              -> (u32, u32, Box<dyn System>, BasicApplication) {
            let system = |_: Read<MockParameter>| ();
            #[allow(trivial_casts)] // Compiler won't coerce `FunctionSystem` to `dyn System` for some reason.
            let boxed_system = Box::new(system.into_system()) as Box<dyn System>;

            let mut app = BasicApplication::default();
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
        #[strategy(arb_function_system())] input: (u32, u32, Box<dyn System>, BasicApplication),
    ) {
        let (segment_size, expected_segment_count, system, app) = input;
        let segment_size = NonZeroU32::new(segment_size).unwrap();

        let segment_iterable = system.try_as_segment_iterable().unwrap();

        let segments = segment_iterable.segments(&Arc::new(app.world), segment_size);

        assert_eq!(expected_segment_count as usize, segments.len())
    }

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
        let expected_components = vec![A(1431), A(123), A(94), A(2)];

        let mut application = BasicApplication::default();

        for expected_component in expected_components.clone() {
            let entity = application.create_entity().unwrap();
            application
                .add_component(entity, expected_component)
                .unwrap();
        }

        let world = Arc::new(application.world);

        let (sequentially_iterated_components, system) =
            set_up_system_that_records_iterated_components();
        let sequential_iterable = system.try_as_sequentially_iterable().unwrap();
        sequential_iterable.run(&world).unwrap();

        let (segment_iterated_components, system) =
            set_up_system_that_records_iterated_components();
        let segmented_iterable = system.try_as_segment_iterable().unwrap();
        let segment_size = NonZeroU32::new(2).unwrap();
        segmented_iterable
            .segments(&world, segment_size)
            .into_iter()
            .for_each(|segment| segment.execute());

        let sequential_components_guard = sequentially_iterated_components.lock().unwrap();
        let sequential_components = sequential_components_guard.iter().cloned().collect_vec();
        let segment_components_guard = segment_iterated_components.lock().unwrap();
        let segment_components = segment_components_guard.iter().cloned().collect_vec();
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

            fn borrow<'world>(
                _world: &'world World,
                _archetypes: &[ArchetypeIndex],
                _system: &'world dyn System,
            ) -> SystemParameterResult<Self::BorrowedData<'world>> {
                unimplemented!()
            }

            unsafe fn fetch_parameter(_borrowed: &mut Self::BorrowedData<'_>) -> Option<Self> {
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

        let result = system.try_as_segment_iterable();

        assert!(result.is_none());
    }

    #[test]
    fn entities_only_query_iterates_over_all_entities() {
        let system_only_querying_entity = (|_: Entity| {}).into_system();
        let mut world = World::default();

        // Create entities with various sets of components...
        let entity0 = world.create_empty_entity().unwrap();
        world.add_component_to_entity(entity0, D).unwrap();
        world.add_component_to_entity(entity0, E).unwrap();
        world.add_component_to_entity(entity0, F).unwrap();
        let entity1 = world.create_empty_entity().unwrap();
        world.add_component_to_entity(entity1, D).unwrap();
        world.add_component_to_entity(entity1, E).unwrap();
        let entity2 = world.create_empty_entity().unwrap();
        world.add_component_to_entity(entity2, D).unwrap();
        let _entity3 = world.create_empty_entity().unwrap();

        let query: Query<(Entity,)> = Query::new(&world, &system_only_querying_entity);

        let queried_entities: Vec<_> = query.into_iter().collect();

        assert_eq!(
            queried_entities.len(),
            4,
            "query should contain all entities, but only contained {:?}",
            queried_entities
        )
    }
}
