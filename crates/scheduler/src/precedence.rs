use ecs::systems::{ComponentAccessDescriptor, System};
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

pub fn find_overlapping_component_accesses(
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

#[cfg(test)]
mod tests {
    use super::*;
    use test_utils::{
        into_system, other_read_a, other_write_a, read_a, read_a_write_b, read_a_write_c, read_ab,
        read_b_write_a, write_a,
    };

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
}
