//! Query filters can be used as [`SystemParameter`]s to narrow down system queries.

use crate::{ComponentAccessDescriptor, Entity, ReadComponentVec, SystemParameter, World};
use std::any::TypeId;
use std::marker::PhantomData;

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
        vec![ComponentAccessDescriptor::Read(TypeId::of::<Component>())]
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
        vec![ComponentAccessDescriptor::Read(TypeId::of::<Component>())]
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
            "# use {ecs::filter::With, ecs::filter::", stringify!($name), "};\n",
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
        pub struct $name<L: SystemParameter, R: SystemParameter> {
            left: PhantomData<L>,
            right: PhantomData<R>,
        }

        impl<L: SystemParameter, R: SystemParameter> SystemParameter for $name<L, R> {
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
pub struct Not<T: SystemParameter> {
    phantom: PhantomData<T>,
}

impl<T: SystemParameter> SystemParameter for Not<T> {
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

/// A query filter that matches any entity.
///
/// # Example
/// ```
/// # use ecs::filter::Any;
/// fn all_entities(_: Any) {
///     println!("Hello from an entity!");
/// }
/// ```
#[derive(Debug)]
pub struct Any {}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Read, Write};
    use proptest::proptest;
    use test_log::test;

    #[derive(Debug)]
    struct A;

    #[derive(Debug)]
    struct B;

    #[derive(Debug)]
    struct C;

    fn test_filter<Filter: SystemParameter>((a, b, c): (bool, bool, bool)) -> Option<Filter> {
        let mut world: World = Default::default();

        let entity: Entity = Default::default();
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

        let mut borrowed = <Filter as SystemParameter>::borrow(&world);
        unsafe { <Filter as SystemParameter>::fetch_parameter(&mut borrowed, entity) }
    }

    macro_rules! identity {
        ($vars:expr, $expr:ty, $expected:literal) => {
            assert_eq!(test_filter::<$expr>($vars).is_some(), $expected);
        };
        ($vars:expr, $lhs:ty, $rhs:ty) => {
            assert_eq!(
                test_filter::<$lhs>($vars).is_some(),
                test_filter::<$rhs>($vars).is_some(),
            );
        };
    }

    proptest! {
        #[test]
        fn test_filter_identities(vars: (bool, bool, bool)) {
            identity!(vars, With<A>, Read<A>);
            identity!(vars, With<A>, Write<A>);

            identity!(vars, Any, true);
            identity!(vars, Not<Any>, false);

            identity!(vars, Without<A>, Not<With<A>>);
            identity!(vars, With<A>, Not<Without<A>>);

            // Involution
            identity!(vars, Not<Not<With<A>>>, With<A>);

            // Dominance
            identity!(vars, Or<With<A>, Any>, true);
            identity!(vars, And<With<A>, Not<Any>>, false);

            // Identity
            identity!(vars, Or<With<A>, Not<Any>>, With<A>);
            identity!(vars, And<With<A>, Any>, With<A>);
            identity!(vars, Xor<With<A>, Not<Any>>, With<A>);

            // Complementarity
            identity!(vars, Or<With<A>, Without<A>>, true);
            identity!(vars, And<With<A>, Without<A>>, false);
            identity!(vars, Xor<With<A>, Without<A>>, true);

            // Idempotence
            identity!(vars, Or<With<A>, With<A>>, With<A>);
            identity!(vars, And<With<A>, With<A>>, With<A>);
            identity!(vars, Xor<With<A>, With<A>>, false);

            // Complementarity
            identity!(vars, Or<With<A>, Without<A>>, true);
            identity!(vars, And<With<A>, Without<A>>, false);
            identity!(vars, Xor<With<A>, Without<A>>, true);

            // Commutativity
            identity!(vars, Or<With<A>, With<B>>, Or<With<B>, With<A>>);
            identity!(vars, And<With<A>, With<B>>, And<With<B>, With<A>>);
            identity!(vars, Xor<With<A>, With<B>>, Xor<With<B>, With<A>>);

            // Associativity
            identity!(vars, Or<Or<With<A>, With<B>>, With<C>>, Or<With<A>, Or<With<B>, With<C>>>);
            identity!(vars, And<And<With<A>, With<B>>, With<C>>, And<With<A>, And<With<B>, With<C>>>);
            identity!(vars, Xor<Xor<With<A>, With<B>>, With<C>>, Xor<With<A>, Xor<With<B>, With<C>>>);

            // Distributivity
            identity!(vars, Or<With<A>, And<With<B>, With<C>>>, And<Or<With<A>, With<B>>, Or<With<A>, With<C>>>);
            identity!(vars, And<With<A>, Or<With<B>, With<C>>>, Or<And<With<A>, With<B>>, And<With<A>, With<C>>>);

            // Absorption
            identity!(vars, And<With<A>, Or<With<A>, With<B>>>, With<A>);

            // De Morgan's laws
            identity!(vars, Or<With<A>, With<B>>, Not<And<Without<A>, Without<B>>>);
            identity!(vars, And<With<A>, With<B>>, Not<Or<Without<A>, Without<B>>>);

            // Xor expressed in terms of And, Or and Not
            identity!(vars, Xor<With<A>, With<B>>, Or<And<With<A>, Without<B>>, And<Without<A>, With<B>>>);
            identity!(vars, Xor<With<A>, With<B>>, And<Or<With<A>, With<B>>, Or<Without<A>, Without<B>>>);
        }
    }
}
