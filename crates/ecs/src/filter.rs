//! Query filters can be used as system parameters to narrow down system queries.

use crate::systems::{
    ComponentAccessDescriptor, SegmentConfig, System, SystemParameter, SystemParameterResult,
    UnitSegment,
};
use crate::{ArchetypeIndex, NoHashHashSet, World};
use std::any::TypeId;
use std::fmt::Debug;
use std::marker::PhantomData;

/// A query filter
pub trait Filter {}

/// A query filter that matches any entity with a given component type.
///
/// # Example
/// ```
/// # use ecs::filter::With;
/// # use ecs::systems::Read;
/// # #[derive(Debug)]
/// # struct Position;
/// # struct Player;
/// fn player_position(position: Read<Position>, _: With<Player>) {
///     println!("A player is at position {:?}.", position);
/// }
/// ```
#[derive(Debug)]
pub struct With<Component: 'static> {
    phantom: PhantomData<Component>,
}

impl<Component: 'static> Default for With<Component> {
    fn default() -> Self {
        Self {
            phantom: PhantomData::default(),
        }
    }
}

impl<Component: Debug + Send + Sync + 'static + Sized> Filter for With<Component> {}
impl<Component: Debug + Send + Sync + 'static + Sized> SystemParameter for With<Component> {
    type BorrowedData<'components> = ();
    type SegmentData = UnitSegment<Self>;

    fn borrow<'world>(
        _: &'world World,
        _: &[ArchetypeIndex],
        _system: &Box<dyn System>,
    ) -> SystemParameterResult<Self::BorrowedData<'world>> {
        Ok(())
    }

    fn split_borrowed_data(
        _: &mut Self::BorrowedData<'_>,
        segment_config: SegmentConfig,
    ) -> Vec<Self::SegmentData> {
        UnitSegment::create_segments(segment_config)
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![]
    }

    fn controls_iteration() -> bool {
        false
    }

    fn base_signature() -> Option<TypeId> {
        Some(TypeId::of::<Component>())
    }

    fn filter(
        _universe: &NoHashHashSet<ArchetypeIndex>,
        world: &World,
    ) -> NoHashHashSet<ArchetypeIndex> {
        world.get_archetype_indices(&[TypeId::of::<Component>()])
    }
}

/// A query filter that matches any entity without a given component type.
///
/// # Example
/// ```
/// # use ecs::filter::{With, Without};
/// # use ecs::systems::Read;
/// # #[derive(Debug)]
/// # struct Position;
/// # #[derive(Debug)]
/// # struct Player;
/// fn non_player_position(position: Read<Position>, _: Without<Player>) {
///     println!("A non-player entity is at position {:?}.", position);
/// }
/// ```
pub type Without<T> = Not<With<T>>;

macro_rules! binary_filter_operation {
    ($name:ident, $op:tt, $op_name:literal, $op_name_lowercase:literal) => {
        /// A query filter that combines two filters using the `
		#[doc = $op_name]
		///` operation.
        ///
        #[doc = concat!(
            "# Example\n```\n",
            "# use ecs::filter::{With, ", stringify!($name), "};\n",
            "# use ecs::systems::Read;\n",
            "# #[derive(Debug)]\n",
            "# struct Position;\n",
            "# #[derive(Debug)]\n",
            "# struct Player;\n",
            "# #[derive(Debug)]\n",
            "# struct Enemy;\n",
            "fn player_", $op_name_lowercase, "_enemy(position: Read<Position>, _: ", stringify!($name), "<With<Player>, With<Enemy>>) {\n",
            "    println!(\"An entity at position {:?} is a player ", $op_name_lowercase ," an enemy.\", position);\n",
            "}\n```",
        )]
        #[derive(Debug)]
        pub struct $name<L: Filter, R: Filter> {
            left: PhantomData<L>,
            right: PhantomData<R>,
        }

        impl<L: Filter, R: Filter> Default for $name<L, R> {
            fn default() -> Self {
                Self {
                    left: PhantomData::default(),
                    right: PhantomData::default(),
                }
            }
        }

        impl<L: Filter, R: Filter> Filter for $name<L, R> {}
        impl<L: Filter + SystemParameter, R: Filter + SystemParameter> SystemParameter for $name<L, R> {
            type BorrowedData<'components> = ();
            type SegmentData = UnitSegment<Self>;

            fn borrow<'world>(
                _: &'world World,
                _: &[ArchetypeIndex],
                _: &Box<dyn System>,
            ) -> SystemParameterResult<Self::BorrowedData<'world>> {
                Ok(())
            }

            fn split_borrowed_data(
                _: &mut Self::BorrowedData<'_>,
                segment_config: SegmentConfig,
            ) -> Vec<Self::SegmentData> {
                UnitSegment::create_segments(segment_config)
            }

            fn component_accesses() -> Vec<ComponentAccessDescriptor> {
                vec![]
            }

            fn controls_iteration() -> bool {
                false
            }

            fn base_signature() -> Option<TypeId> {
                None
            }

            fn filter(
                universe: &NoHashHashSet<ArchetypeIndex>,
                world: &World,
            ) -> NoHashHashSet<ArchetypeIndex> {
                &<L as SystemParameter>::filter(universe, world)
                    $op &<R as SystemParameter>::filter(universe, world)
            }
        }
    }
}

binary_filter_operation!(And, &, "And", "and");
binary_filter_operation!(Or, |, "Or", "or");
binary_filter_operation!(Xor, ^, "Xor", "xor");

/// A query filter that inverts another filter.
///
/// # Example
/// ```
/// # use ecs::filter::{Not, With};
/// # use ecs::systems::Read;
/// # #[derive(Debug)]
/// # struct Position;
/// #[derive(Debug)]
/// # struct Player;
/// fn non_player_position(position: Read<Position>, _: Not<With<Player>>) {
///     println!("A non-player entity is at position {:?}.", position);
/// }
/// ```
#[derive(Debug)]
pub struct Not<F: Filter> {
    phantom: PhantomData<F>,
}

impl<F: Filter> Default for Not<F> {
    fn default() -> Self {
        Self {
            phantom: PhantomData::default(),
        }
    }
}

impl<T: Filter> Filter for Not<T> {}
impl<T: Filter + SystemParameter> SystemParameter for Not<T> {
    type BorrowedData<'components> = ();
    type SegmentData = UnitSegment<Self>;

    fn borrow<'world>(
        _: &'world World,
        _: &[ArchetypeIndex],
        _system: &Box<dyn System>,
    ) -> SystemParameterResult<Self::BorrowedData<'world>> {
        Ok(())
    }

    fn split_borrowed_data(
        _: &mut Self::BorrowedData<'_>,
        segment_config: SegmentConfig,
    ) -> Vec<Self::SegmentData> {
        UnitSegment::create_segments(segment_config)
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![]
    }

    fn controls_iteration() -> bool {
        false
    }

    fn base_signature() -> Option<TypeId> {
        None
    }

    fn filter(
        universe: &NoHashHashSet<ArchetypeIndex>,
        world: &World,
    ) -> NoHashHashSet<ArchetypeIndex> {
        universe
            .difference(&<T as SystemParameter>::filter(universe, world))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::System;
    use crate::systems::{IntoSystem, Read, Write};
    use color_eyre::Report;
    use std::sync::Arc;
    use test_log::test;
    use test_strategy::proptest;

    /// A query filter that matches any entity.
    /// Only used for testing.
    #[derive(Debug, Default)]
    pub struct Any {}

    impl Filter for Any {}
    impl SystemParameter for Any {
        type BorrowedData<'components> = ();
        type SegmentData = UnitSegment<Self>;

        fn borrow<'world>(
            _: &'world World,
            _: &[ArchetypeIndex],
            _: &Box<dyn System>,
        ) -> SystemParameterResult<Self::BorrowedData<'world>> {
            Ok(())
        }

        fn split_borrowed_data(
            _: &mut Self::BorrowedData<'_>,
            segment_config: SegmentConfig,
        ) -> Vec<Self::SegmentData> {
            UnitSegment::create_segments(segment_config)
        }

        fn component_accesses() -> Vec<ComponentAccessDescriptor> {
            vec![]
        }

        fn controls_iteration() -> bool {
            false
        }

        fn base_signature() -> Option<TypeId> {
            None
        }

        fn filter(
            universe: &NoHashHashSet<ArchetypeIndex>,
            _: &World,
        ) -> NoHashHashSet<ArchetypeIndex> {
            universe.clone()
        }
    }

    #[derive(Debug)]
    struct A;

    #[derive(Debug)]
    struct B;

    #[derive(Debug)]
    struct C;

    #[derive(Debug)]
    struct TestResult(bool);

    fn test_filter<Filter: SystemParameter + 'static>(
        a: bool,
        b: bool,
        c: bool,
    ) -> Result<bool, Report> {
        let mut world = World::default();

        let entity = world.create_entity((TestResult(false),))?;

        if a {
            world.add_component_to_entity(entity, A)?;
        }
        if b {
            world.add_component_to_entity(entity, B)?;
        }
        if c {
            world.add_component_to_entity(entity, C)?;
        }

        let system = |mut test_result: Write<TestResult>, _: Filter| {
            test_result.0 = true;
        };

        let world = Arc::new(world);

        let function_system = system.into_system();
        function_system
            .try_as_sequentially_iterable()
            .unwrap()
            .run(&world)?;

        let archetypes: Vec<ArchetypeIndex> = world
            .get_archetype_indices(&[TypeId::of::<TestResult>()])
            .into_iter()
            .collect();

        let boxed_system: Box<dyn System> = Box::new(function_system);
        let mut borrowed =
            <Read<TestResult> as SystemParameter>::borrow(&world, &archetypes, &boxed_system)?;
        let segments = <Read<TestResult> as SystemParameter>::split_borrowed_data(
            &mut borrowed,
            SegmentConfig::Single,
        );

        if let Some(result) = segments[0].clone().into_iter().next() {
            Ok(result.0)
        } else {
            panic!("Could not fetch the test result.")
        }
    }

    macro_rules! logically_eq {
        // Test with zero variables and compare with boolean
        ($expr:ty, $expected:literal) => {
            assert_eq!(
                test_filter::<$expr>(false, false, false).unwrap(),
                $expected
            );
        };
        // Test with three variables a, b and c
        (($a:expr, $b:expr, $c:expr), $lhs:ty, $rhs:ty) => {
            assert_eq!(
                test_filter::<$lhs>($a, $b, $c).unwrap(),
                test_filter::<$rhs>($a, $b, $c).unwrap(),
            );
        };
        // Test with two variables a and b
        (($a:expr, $b:expr), $lhs:ty, $rhs:ty) => {
            assert_eq!(
                test_filter::<$lhs>($a, $b, false).unwrap(),
                test_filter::<$rhs>($a, $b, false).unwrap(),
            );
        };
        // Test with one variable a
        ($a:expr, $lhs:ty, $rhs:ty) => {
            assert_eq!(
                test_filter::<$lhs>($a, false, false).unwrap(),
                test_filter::<$rhs>($a, false, false).unwrap(),
            );
        };
    }

    #[proptest]
    fn basic_test(a: bool) {
        logically_eq!(a, With<A>, Read<A>);

        logically_eq!(Any, true);
        logically_eq!(Not<Any>, false);

        logically_eq!(a, Without<A>, Not<With<A>>);
        logically_eq!(a, With<A>, Not<Without<A>>);
    }

    #[proptest]
    fn test_involution(a: bool) {
        logically_eq!(a, Not<Not<With<A>>>, With<A>);
    }

    #[proptest]
    fn test_dominance(a: bool) {
        logically_eq!(a, Or<With<A>, Any>, Any);
        logically_eq!(a, And<With<A>, Not<Any>>, Not<Any>);
    }

    #[proptest]
    fn test_identity_elem(a: bool) {
        logically_eq!(a, Or<With<A>, Not<Any>>, With<A>);
        logically_eq!(a, And<With<A>, Any>, With<A>);
        logically_eq!(a, Xor<With<A>, Not<Any>>, With<A>);
    }

    #[proptest]
    fn test_complementarity(a: bool) {
        logically_eq!(a, Or<With<A>, Without<A>>, Any);
        logically_eq!(a, And<With<A>, Without<A>>, Not<Any>);
        logically_eq!(a, Xor<With<A>, Without<A>>, Any);
    }

    #[proptest]
    fn test_idempotence(a: bool) {
        logically_eq!(a, Or<With<A>, With<A>>, With<A>);
        logically_eq!(a, And<With<A>, With<A>>, With<A>);
        logically_eq!(a, Xor<With<A>, With<A>>, Not<Any>);
    }

    #[proptest]
    fn test_commutativity(a: bool, b: bool) {
        logically_eq!((a, b), Or<With<A>, With<B>>, Or<With<B>, With<A>>);
        logically_eq!((a, b), And<With<A>, With<B>>, And<With<B>, With<A>>);
        logically_eq!((a, b), Xor<With<A>, With<B>>, Xor<With<B>, With<A>>);
    }

    #[proptest]
    fn test_associativity(a: bool, b: bool) {
        logically_eq!(
            (a, b),
            Or<Or<With<A>, With<B>>, With<C>>,
            Or<With<A>, Or<With<B>, With<C>>>
        );
        logically_eq!(
            (a, b),
            And<And<With<A>, With<B>>, With<C>>,
            And<With<A>, And<With<B>, With<C>>>
        );
        logically_eq!(
            (a, b),
            Xor<Xor<With<A>, With<B>>, With<C>>,
            Xor<With<A>, Xor<With<B>, With<C>>>
        );
    }

    #[proptest]
    fn test_distributivity(a: bool, b: bool, c: bool) {
        logically_eq!(
            (a, b, c),
            Or<With<A>, And<With<B>, With<C>>>,
            And<Or<With<A>, With<B>>, Or<With<A>, With<C>>>
        );
        logically_eq!(
            (a, b, c),
            And<With<A>, Or<With<B>, With<C>>>,
            Or<And<With<A>, With<B>>, And<With<A>, With<C>>>
        );
    }

    #[proptest]
    fn test_absorption(a: bool) {
        logically_eq!(a, And<With<A>, Or<With<A>, With<B>>>, With<A>);
    }

    #[proptest]
    fn test_de_morgans_laws(a: bool, b: bool) {
        logically_eq!(
            (a, b),
            Or<With<A>, With<B>>,
            Not<And<Without<A>, Without<B>>>
        );
        logically_eq!(
            (a, b),
            And<With<A>, With<B>>,
            Not<Or<Without<A>, Without<B>>>
        );
    }

    #[proptest]
    fn test_xor_with_and_or_and_not(a: bool, b: bool) {
        logically_eq!(
            (a, b),
            Xor<With<A>, With<B>>,
            Or<And<With<A>, Without<B>>, And<Without<A>, With<B>>>
        );
        logically_eq!(
            (a, b),
            Xor<With<A>, With<B>>,
            And<Or<With<A>, With<B>>, Or<Without<A>, Without<B>>>
        );
    }

    macro_rules! implies {
        ($p:ty, $q:ty) => { Or<Not<$p>, $q> }
    }

    #[proptest]
    fn test_transitivity(a: bool, b: bool, c: bool) {
        logically_eq!(
            (a, b, c),
            implies!(
                And<And<With<A>, With<B>>, And<With<B>, With<C>>>,
                And<With<A>, With<C>>
            ),
            Any
        );
    }
}
