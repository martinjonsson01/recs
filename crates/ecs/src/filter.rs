//! Query filters can be used as system parameters to narrow down system queries.

use crate::{ComponentAccessDescriptor, Entity, ReadComponentVec, SystemParameter, World};
use std::marker::PhantomData;

/// A query filter
pub trait Filter {}

/// A query filter that matches any entity with a given component type.
///
/// # Example
/// ```
/// # use ecs::filter::With;
/// # use ecs::Read;
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

impl<Component: Send + Sync + 'static + Sized> Filter for With<Component> {}
impl<Component: Send + Sync + 'static + Sized> SystemParameter for With<Component> {
    type BorrowedData<'components> = ReadComponentVec<'components, Component>;

    fn borrow(world: &World) -> Self::BorrowedData<'_> {
        world.borrow_component_vec::<Component>()
    }

    unsafe fn fetch_parameter(
        borrowed: &mut Self::BorrowedData<'_>,
        entity: Entity,
    ) -> Option<Self> {
        if let Some(component_vec) = borrowed {
            if let Some(Some(_)) = component_vec.get(entity.id) {
                return Some(Self {
                    phantom: Default::default(),
                });
            }
        }
        None
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![ComponentAccessDescriptor::read::<Component>()]
    }
}

/// A query filter that matches any entity without a given component type.
///
/// # Example
/// ```
/// # use ecs::filter::{With, Without};
/// # use ecs::Read;
/// # #[derive(Debug)]
/// # struct Position;
/// # struct Player;
/// fn non_player_position(position: Read<Position>, _: Without<Player>) {
///     println!("A non-player entity is at position {:?}.", position);
/// }
/// ```
#[derive(Debug)]
pub struct Without<Component: 'static> {
    phantom: PhantomData<Component>,
}

impl<Component: Send + Sync + 'static + Sized> Filter for Without<Component> {}
impl<Component: Send + Sync + 'static + Sized> SystemParameter for Without<Component> {
    type BorrowedData<'components> = ReadComponentVec<'components, Component>;

    fn borrow(world: &World) -> Self::BorrowedData<'_> {
        world.borrow_component_vec::<Component>()
    }

    unsafe fn fetch_parameter(
        borrowed: &mut Self::BorrowedData<'_>,
        entity: Entity,
    ) -> Option<Self> {
        if let Some(component_vec) = borrowed {
            if let Some(Some(_)) = component_vec.get(entity.id) {
                return None;
            }
        }
        Some(Self {
            phantom: Default::default(),
        })
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        vec![ComponentAccessDescriptor::read::<Component>()]
    }
}

macro_rules! binary_filter_operation {
    ($name:ident, $op:tt, $op_name:literal, $op_name_lowercase:literal) => {
        /// A query filter that combines two filters using the `
		#[doc = $op_name]
		///` operation.
        ///
        #[doc = concat!(
            "# Example\n```\n",
            "# use ecs::filter::{With, ", stringify!($name), "};\n",
            "# use ecs::Read;\n",
            "# #[derive(Debug)]\n",
            "# struct Position;\n",
            "# struct Player;\n",
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

        impl<L: Filter, R: Filter> Filter for $name<L, R> {}
        impl<L: Filter + SystemParameter, R: Filter + SystemParameter> SystemParameter for $name<L, R> {
            type BorrowedData<'components> = (
                <L as SystemParameter>::BorrowedData<'components>,
                <R as SystemParameter>::BorrowedData<'components>,
            );

            fn borrow(world: &World) -> Self::BorrowedData<'_> {
                (
                    <L as SystemParameter>::borrow(world),
                    <R as SystemParameter>::borrow(world),
                )
            }

            unsafe fn fetch_parameter(
                borrowed: &mut Self::BorrowedData<'_>,
                entity: Entity,
            ) -> Option<Self> {
                let (left_borrow, right_borrow) = borrowed;
                let left = <L as SystemParameter>::fetch_parameter(left_borrow, entity);
                let right = <R as SystemParameter>::fetch_parameter(right_borrow, entity);
                (left.is_some() $op right.is_some()).then(|| Self {
                    left: PhantomData::default(),
                    right: PhantomData::default(),
                })
            }

            fn component_accesses() -> Vec<ComponentAccessDescriptor> {
                [
                    <L as SystemParameter>::component_accesses(),
                    <R as SystemParameter>::component_accesses(),
                ]
                .concat()
            }
        }
    }
}

binary_filter_operation!(And, &&, "And", "and");
binary_filter_operation!(Or, ||, "Or", "or");
binary_filter_operation!(Xor, ^, "Xor", "xor");

/// A query filter that inverts another filter.
///
/// # Example
/// ```
/// # use ecs::filter::{Not, With};
/// # use ecs::Read;
/// # #[derive(Debug)]
/// # struct Position;
/// # struct Player;
/// fn non_player_position(position: Read<Position>, _: Not<With<Player>>) {
///     println!("A non-player entity is at position {:?}.", position);
/// }
/// ```
#[derive(Debug)]
pub struct Not<T: Filter> {
    phantom: PhantomData<T>,
}

impl<T: Filter> Filter for Not<T> {}
impl<T: Filter + SystemParameter> SystemParameter for Not<T> {
    type BorrowedData<'components> = <T as SystemParameter>::BorrowedData<'components>;

    fn borrow(world: &World) -> Self::BorrowedData<'_> {
        <T as SystemParameter>::borrow(world)
    }

    unsafe fn fetch_parameter(
        borrowed: &mut Self::BorrowedData<'_>,
        entity: Entity,
    ) -> Option<Self> {
        if <T as SystemParameter>::fetch_parameter(borrowed, entity).is_some() {
            None
        } else {
            Some(Self {
                phantom: PhantomData::default(),
            })
        }
    }

    fn component_accesses() -> Vec<ComponentAccessDescriptor> {
        <T as SystemParameter>::component_accesses()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Read;
    use test_log::test;
    use test_strategy::proptest;

    /// A query filter that matches any entity.
    /// Only used for testing.
    #[derive(Debug)]
    pub struct Any {}

    impl Filter for Any {}
    impl SystemParameter for Any {
        type BorrowedData<'components> = ();

        fn borrow(_world: &World) -> Self::BorrowedData<'_> {}

        unsafe fn fetch_parameter(
            _borrowed: &mut Self::BorrowedData<'_>,
            _entity: Entity,
        ) -> Option<Self> {
            Some(Self {})
        }

        fn component_accesses() -> Vec<ComponentAccessDescriptor> {
            vec![]
        }
    }

    #[derive(Debug)]
    struct A;

    #[derive(Debug)]
    struct B;

    #[derive(Debug)]
    struct C;

    fn test_filter<P: SystemParameter>(a: bool, b: bool, c: bool) -> Option<P> {
        let mut world = World::default();

        let entity = Entity::default();
        world.entities.push(entity);

        if a {
            world.create_component_vec_and_add(entity, A);
        }
        if b {
            world.create_component_vec_and_add(entity, B);
        }
        if c {
            world.create_component_vec_and_add(entity, C);
        }

        let mut borrowed = <P as SystemParameter>::borrow(&world);
        // SAFETY: This is safe because the result from fetch_parameter will not outlive borrowed
        unsafe { <P as SystemParameter>::fetch_parameter(&mut borrowed, entity) }
    }

    macro_rules! logically_eq {
        // Test with zero variables and compare with boolean
        ($expr:ty, $expected:literal) => {
            assert_eq!(
                test_filter::<$expr>(false, false, false).is_some(),
                $expected
            );
        };
        // Test with three variables a, b and c
        (($a:expr, $b:expr, $c:expr), $lhs:ty, $rhs:ty) => {
            assert_eq!(
                test_filter::<$lhs>($a, $b, $c).is_some(),
                test_filter::<$rhs>($a, $b, $c).is_some(),
            );
        };
        // Test with two variables a and b
        (($a:expr, $b:expr), $lhs:ty, $rhs:ty) => {
            assert_eq!(
                test_filter::<$lhs>($a, $b, false).is_some(),
                test_filter::<$rhs>($a, $b, false).is_some(),
            );
        };
        // Test with one variable a
        ($a:expr, $lhs:ty, $rhs:ty) => {
            assert_eq!(
                test_filter::<$lhs>($a, false, false).is_some(),
                test_filter::<$rhs>($a, false, false).is_some(),
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
