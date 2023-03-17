use ecs::{ComponentAccessDescriptor, System};
use itertools::Itertools;
use std::cmp::Ordering;

/// Something that can be ordered with respect to precedence.
pub trait Orderable {
    /// Returns the [`Precedence`] between `self` and `other`.
    ///
    /// For example, if `self` precedes `other`, this would return [`Precedence::Before`].
    fn precedence_to(&self, other: &Self) -> Precedence;
}

/// Describes a relation between two items in a precedence-relation.
#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Default)]
pub enum Precedence {
    /// It has to happen before, i.e. it precedes the other.
    Before,
    /// There is no specific precedence between the items, they're considered equal.
    #[default]
    Equal,
    /// It has to happen after, i.e. it succeeds the other.
    After,
}

impl From<Precedence> for Ordering {
    fn from(value: Precedence) -> Self {
        match value {
            Precedence::Before => Ordering::Less,
            Precedence::Equal => Ordering::Equal,
            Precedence::After => Ordering::Greater,
        }
    }
}

impl Orderable for dyn System + '_ {
    // This could be optimized using memoization of system precedences
    // (since system parameters don't change during runtime)
    // but until benchmarks reveal that this has any impact on performance, we'll leave it.
    fn precedence_to(&self, other: &Self) -> Precedence {
        let overlapping_components = find_overlapping_component_accesses(self, other);

        if overlapping_components.is_empty() {
            return Precedence::Equal;
        }

        let both_read_all = overlapping_components
            .iter()
            .all(|(a, b)| a.is_read() && b.is_read());
        if both_read_all {
            return Precedence::Equal;
        }

        let other_writes_any = overlapping_components.iter().any(|(_, b)| b.is_write());
        if other_writes_any {
            return Precedence::After;
        }

        let other_reads_any = overlapping_components
            .iter()
            .any(|(a, b)| a.is_write() && b.is_read());
        if other_reads_any {
            return Precedence::Before;
        }

        Precedence::Equal
    }
}

fn find_overlapping_component_accesses(
    system: &dyn System,
    other: &dyn System,
) -> Vec<(ComponentAccessDescriptor, ComponentAccessDescriptor)> {
    let components = system.component_accesses();
    let other_components = other.component_accesses();

    let overlapping_components: Vec<_> = components
        .into_iter()
        .cartesian_product(other_components.into_iter())
        .filter(|(a, b)| a.component_type() == b.component_type())
        .collect();
    overlapping_components
}

/// Returns the [`orderables`] sorted by their relative [`Precedence`].
#[allow(dead_code)] // Will be used by schedule generation. -- Remove annotation once implemented!
pub fn order_by_precedence<'a, OrderedItem>(
    orderables: impl IntoIterator<Item = &'a OrderedItem>,
) -> std::vec::IntoIter<&'a OrderedItem>
where
    OrderedItem: Orderable + 'a + ?Sized,
{
    orderables
        .into_iter()
        .sorted_by(|a, b| a.precedence_to(b).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecs::{IntoSystem, Read, SystemParameters, Write};
    use proptest::collection::hash_set;
    use proptest::prop_compose;
    use std::collections::HashSet;
    use test_strategy::proptest;

    #[derive(Debug, Default)]
    pub struct A(i32);
    #[derive(Debug, Default)]
    pub struct B(String);
    #[derive(Debug, Default)]
    pub struct C(f32);

    fn read_a(_: Read<A>) {}
    fn other_read_a(_: Read<A>) {}
    fn write_a(_: Write<A>) {}
    fn other_write_a(_: Write<A>) {}

    fn read_b_write_a(_: Read<B>, _: Write<A>) {}
    fn read_a_write_c(_: Read<A>, _: Write<C>) {}

    fn read_a_write_b(_: Read<A>, _: Write<B>) {}
    fn read_ab(_: Read<A>, _: Read<B>) {}

    fn into_system<F: IntoSystem<Parameters>, Parameters: SystemParameters>(
        function: F,
    ) -> Box<dyn System> {
        Box::new(function.into_system())
    }

    #[test]
    fn system_writing_to_component_precedes_system_reading_from_component() {
        let read_system = into_system(read_a);
        let write_system = into_system(write_a);

        let ordering0 = read_system.precedence_to(write_system.as_ref());
        let ordering1 = write_system.precedence_to(read_system.as_ref());

        // read_system > write_system
        assert_eq!(Precedence::After, ordering0);
        // write_system < read_system
        assert_eq!(Precedence::Before, ordering1);
    }

    #[test]
    fn systems_writing_to_component_precede_each_other() {
        let write_system0 = into_system(write_a);
        let write_system1 = into_system(other_write_a);

        let ordering0 = write_system0.precedence_to(write_system1.as_ref());
        let ordering1 = write_system1.precedence_to(write_system0.as_ref());

        // write_system > write_system
        assert_eq!(Precedence::After, ordering0);
        assert_eq!(Precedence::After, ordering1);
    }

    #[test]
    fn systems_reading_from_component_are_of_equal_precedence() {
        let read_system0 = into_system(read_a);
        let read_system1 = into_system(other_read_a);

        let ordering0 = read_system0.precedence_to(read_system1.as_ref());
        let ordering1 = read_system1.precedence_to(read_system0.as_ref());

        assert_eq!(Precedence::Equal, ordering0);
        assert_eq!(Precedence::Equal, ordering1);
    }

    #[test]
    fn system_reading_from_many_and_writing_to_component_precedes_system_read_from_component() {
        let many_system = into_system(read_b_write_a);
        let read_system = into_system(read_a);

        let ordering0 = many_system.precedence_to(read_system.as_ref());
        let ordering1 = read_system.precedence_to(many_system.as_ref());

        assert_eq!(Precedence::Before, ordering0);
        assert_eq!(Precedence::After, ordering1);
    }

    #[test]
    fn system_reading_from_many_and_writing_to_component_precedes_system_writing_to_many_and_reading_from_component(
    ) {
        let many_parameters_but_single_write = into_system(read_b_write_a);
        let many_parameters_but_single_read = into_system(read_a_write_c);

        let ordering0 = many_parameters_but_single_write
            .precedence_to(many_parameters_but_single_read.as_ref());
        let ordering1 = many_parameters_but_single_read
            .precedence_to(many_parameters_but_single_write.as_ref());

        assert_eq!(Precedence::Before, ordering0);
        assert_eq!(Precedence::After, ordering1);
    }

    #[test]
    fn system_writing_precedes_system_reading_even_if_both_also_read_other_component() {
        let read_a_write_b = into_system(read_a_write_b);
        let read_ab = into_system(read_ab);

        let ordering0 = read_a_write_b.precedence_to(read_ab.as_ref());
        let ordering1 = read_ab.precedence_to(read_a_write_b.as_ref());

        assert_eq!(Precedence::Before, ordering0);
        assert_eq!(Precedence::After, ordering1);
    }

    fn arb_systems() -> Vec<Box<dyn System>> {
        vec![
            into_system(read_a),
            into_system(other_read_a),
            into_system(write_a),
            into_system(other_write_a),
            into_system(read_a_write_b),
            into_system(read_ab),
        ]
    }

    prop_compose! {
        fn arb_system(system_count: usize)
                     (system_index in 0..system_count)
                     -> Box<dyn System> {
            let mut systems = arb_systems();
            systems.remove(system_index)
        }
    }

    prop_compose! {
        fn arb_ordered_systems(max_system_count: usize)
                              (systems_count in 1..=max_system_count)
                              (systems in hash_set(arb_system(systems_count), systems_count))
                              -> HashSet<Box<dyn System>> {
            systems
        }
    }

    /// NOTE: this assertion does _not_ hold for systems that mutually read and write to each others
    /// components.
    ///
    /// For example, [`read_a_write_b`] and [`write_a_read_b`] fails this test because:
    /// `read_a_write_b.precedence_to(write_a_read_b) == Precedence::After`
    /// but the other way around results in the same:
    /// `write_a_read_b.precedence_to(read_a_write_b) == Precedence::After`
    ///
    /// This is because the order of those two systems is arbitrary, the only important
    /// invariant is that they can't execute simultaneously.
    #[proptest]
    fn systems_are_ordered_according_to_precedence(
        #[strategy(arb_ordered_systems(6))] systems: HashSet<Box<dyn System>>,
    ) {
        let systems = systems.iter().map(|system| system.as_ref());

        let ordered = order_by_precedence(systems);

        let mut previous: Option<&dyn System> = None;
        for system in ordered {
            if let Some(previous_system) = previous {
                let precedence = previous_system.precedence_to(system);
                let before_or_equal =
                    precedence == Precedence::Before || precedence == Precedence::Equal;
                // It's okay if they're not in ascending order when they write to same component
                // since there is no stable ordering between two systems writing to same component.
                assert!(before_or_equal || write_to_same_component(previous_system, system));
            }
            previous = Some(system);
        }
    }

    fn write_to_same_component(previous: &dyn System, current: &dyn System) -> bool {
        let overlapping_components = find_overlapping_component_accesses(previous, current);

        overlapping_components
            .iter()
            .all(|(a, b)| a.is_write() && b.is_write())
    }
}
