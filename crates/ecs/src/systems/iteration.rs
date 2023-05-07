//! Different ways to iterate over all queried entities in a [`System`].

use super::*;
use crate::World;
use itertools::izip;
use std::any::Any;

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
                    let archetypes = <($([<P$parameter>],)*) as SystemParameters>::get_archetype_indices(world);

                    let boxed_system: Box<dyn System> = Box::new(self.clone());
                    $(let mut [<borrowed_$parameter>] = [<P$parameter>]::borrow(world, &archetypes, &boxed_system).map_err(SystemError::MissingParameter)?;)*
                    let mut segments = ($([<P$parameter>]::split_borrowed_data(&mut [<borrowed_$parameter>], SegmentConfig::Single),)*);

                    let query: Query<($([<P$parameter>],)*)> = Query {
                        segments: &mut segments,
                        world,
                        archetypes: &archetypes,
                        iterate_once: !($([<P$parameter>]::controls_iteration())||*),
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

/// Execution of a single [`System`] in a multiple segments.
pub trait SegmentIterable: Send + Sync {
    /// Borrows the data necessary for segmentation.
    fn borrow<'world>(&self, world: &'world World) -> SystemResult<Box<dyn Any + 'world>>;

    /// Divides the iteration up into segments with target size specified by `segment`.
    ///
    /// # Safety
    /// The resulting [`SystemSegment`]s contain data borrowed from `borrowed`, meaning
    /// that they have to be dropped before `borrowed` is.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::num::NonZeroU32;
    /// # use std::sync::Arc;
    /// # use ecs::systems::{IntoSystem, Read, SegmentSize, System, SystemError};
    /// # use ecs::systems::iteration::SystemSegment;
    /// # use ecs::World;
    /// # let system = (|_: Read<i32>| ()).into_system();
    /// # let segment_iterable = system.try_as_segment_iterable().unwrap();
    /// # let world = World::default();
    /// # let world = unsafe{ std::mem::transmute(&world) };
    ///
    /// let mut borrowed = segment_iterable.borrow(world)?;
    /// // SAFETY: `borrowed` is dropped after `segments`.
    /// let segments = unsafe { segment_iterable.segments(borrowed.as_mut(), SegmentSize::Auto)? };
    ///
    /// for segment in segments {
    ///     segment.execute();
    /// }
    ///
    /// drop(borrowed);
    ///
    /// # Ok::<(), SystemError>(())
    /// ```
    unsafe fn segments(
        &self,
        borrowed: &mut dyn Any,
        segment: SegmentSize,
    ) -> SystemResult<Vec<SystemSegment>>;
}

impl<Function> SegmentIterable for FunctionSystem<Function, ()>
where
    Function: Fn() + Send + Sync + 'static,
{
    fn borrow<'world>(&self, _world: &'world World) -> SystemResult<Box<dyn Any + 'world>> {
        Ok(Box::new(()))
    }

    unsafe fn segments<'world>(
        &self,
        _borrowed: &mut dyn Any,
        _segment: SegmentSize,
    ) -> SystemResult<Vec<SystemSegment>> {
        let function = Arc::clone(&self.function);
        let execution = move || {
            function();
        };
        let segment = SystemSegment {
            system_name: self.function_name,
            executable: Box::new(execution),
        };
        Ok(vec![segment])
    }
}

/// A part of the full execution of a single [`FunctionSystem`].
pub struct SystemSegment {
    /// The name of the [`System`] this is a segment of.
    pub system_name: &'static str,
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

macro_rules! impl_parallel_iterable_system {
    ($($parameter:expr),*) => {
        paste! {
            impl<Function, $([<P$parameter>]: SystemParameter + 'static,)*> SegmentIterable
                for FunctionSystem<Function, ($([<P$parameter>],)*)>
            where
                Function: Fn($([<P$parameter>],)*) + Send + Sync + 'static,
            {
                fn borrow<'world>(&self, world: &'world World) -> SystemResult<Box<dyn Any + 'world>> {
                    let archetypes = <($([<P$parameter>],)*) as SystemParameters>::get_archetype_indices(world);

                    let entity_count: usize = archetypes
                        .iter()
                        .map(|&archetype_index| {
                            world
                                .archetypes
                                .get(archetype_index)
                                .expect("archetype_index should always be valid")
                                .entity_count()
                        })
                        .sum();

                    let boxed_system: Box<dyn System> = Box::new(self.clone());
                    $(let [<borrowed_$parameter>] = [<P$parameter>]::borrow(world, &archetypes, &boxed_system).map_err(SystemError::MissingParameter)?;)*

                    let borrowed_parameters = ($([<borrowed_$parameter>],)*);

                    Ok(Box::new((borrowed_parameters, entity_count)))
                }

                unsafe fn segments(
                    &self,
                    borrowed: &mut dyn Any,
                    segment_size: SegmentSize,
                ) -> SystemResult<Vec<SystemSegment>> {

                    let (borrowed_parameters, entity_count) = borrowed
                        .downcast_mut::<(($([<P$parameter>]::BorrowedData<'_>,)*), usize)>()
                        .ok_or(SystemError::BorrowedData)?;

                    let segment_config = match segment_size {
                        SegmentSize::Single => { SegmentConfig::Single }
                        SegmentSize::Size(size) => {
                            let segment_size = size as usize;
                            SegmentConfig::Size {
                                segment_size,
                                segment_count: calculate_segment_count(*entity_count, segment_size)
                            }
                        }
                        SegmentSize::Auto => {
                            let segment_size = calculate_auto_segment_size(*entity_count);
                            SegmentConfig::Size {
                                segment_size,
                                segment_count: calculate_segment_count(*entity_count, segment_size)
                            }
                        }
                    };

                    $(let mut [<segments_$parameter>] = [<P$parameter>]::split_borrowed_data(&mut borrowed_parameters.$parameter, segment_config);)*

                    let mut system_segments = Vec::with_capacity(segment_config.get_segment_count());

                    // Ignore warning about unnecessary parentheses for single-parameter systems.
                    // This is because izip! does not return a single-element tuple.
                    #[allow(unused_parens)]
                    if $([<P$parameter>]::controls_iteration() )||* {
                        for ($([<segment_$parameter>]),*) in izip!($([<segments_$parameter>],)*) {
                            let function = Arc::clone(&self.function);
                            //$(let mut [<segment_$parameter>] = mem::transmute([<segment_$parameter>]);)*

                            let execution = move || {
                                for ($([<parameter_$parameter>]),*) in izip!($([<segment_$parameter>].into_iter(),)*) {
                                    (function)($([<parameter_$parameter>],)*);
                                }
                            };
                            let segment = SystemSegment {
                                system_name: self.function_name,
                                executable: Box::new(execution),
                            };

                            system_segments.push(segment);
                        }
                    } else if let Some(($([<parameter_$parameter>]),*)) = izip!($([<segments_$parameter>].remove(0).into_iter(),)*).next() {
                        // This will be run on the calling thread.
                        // For some reason, turning it into a SystemSegment and running it
                        // on the WorkerPool causes access violations.
                        (self.function)($([<parameter_$parameter>],)*);
                    }

                    Ok(system_segments)
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
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use test_utils::{A, B};
    use test_utils::{D, E, F};

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
            application.create_entity((expected_component,)).unwrap();
        }

        let world = application.world;

        let (sequentially_iterated_components, system) =
            set_up_system_that_records_iterated_components();
        let sequential_iterable = system.try_as_sequentially_iterable().unwrap();
        sequential_iterable.run(&world).unwrap();

        let (segment_iterated_components, system) =
            set_up_system_that_records_iterated_components();
        let segmented_iterable = system.try_as_segment_iterable().unwrap();
        let segment_size = SegmentSize::Size(2);
        let world = unsafe {
            // SAFETY: world is not dropped until tasks have been executed.
            mem::transmute(&world)
        };
        let mut borrowed = segmented_iterable.borrow(world).unwrap();
        // SAFETY: borrowed is dropped after segments have been executed.
        unsafe {
            segmented_iterable
                .segments(borrowed.as_mut(), segment_size)
                .unwrap()
                .into_iter()
                .for_each(|segment| segment.execute());
        }

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
        #[derive(Debug, Default)]
        struct NonParallelParameter;

        impl SystemParameter for NonParallelParameter {
            type BorrowedData<'components> = ();
            type SegmentData = UnitSegment<Self>;

            fn borrow<'world>(
                _world: &'world World,
                _archetypes: &[ArchetypeIndex],
                _system: &Box<dyn System>,
            ) -> SystemParameterResult<Self::BorrowedData<'world>> {
                unimplemented!()
            }

            fn split_borrowed_data(
                _: &mut Self::BorrowedData<'_>,
                _: SegmentConfig,
            ) -> Vec<Self::SegmentData> {
                unimplemented!()
            }

            fn component_accesses() -> Vec<ComponentAccessDescriptor> {
                unimplemented!()
            }

            fn controls_iteration() -> bool {
                unimplemented!()
            }

            fn base_signature() -> Option<TypeId> {
                unimplemented!()
            }

            fn supports_parallelization() -> bool {
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
        world.create_entity((D, E, F)).unwrap();
        world.create_entity((D, E)).unwrap();
        world.create_entity((D,)).unwrap();
        world.create_empty_entity().unwrap();

        let archetypes = <(Entity,) as SystemParameters>::get_archetype_indices(&world);

        let boxed_system: Box<dyn System> = Box::new(system_only_querying_entity);
        let mut borrowed = Entity::borrow(&world, &archetypes, &boxed_system).unwrap();
        let mut segments = Entity::split_borrowed_data(&mut borrowed, SegmentConfig::Single);

        let query: Query<(Entity,)> = Query {
            segments: unsafe { mem::transmute(&mut segments) }, /* SAFETY: query is dropped before segments */
            world: &world,
            archetypes: &archetypes,
            iterate_once: !Entity::controls_iteration(),
        };

        let queried_entities: Vec<_> = query.into_iter().collect();

        assert_eq!(
            queried_entities.len(),
            4,
            "query should contain all entities, but only contained {:?}",
            queried_entities
        )
    }
}
