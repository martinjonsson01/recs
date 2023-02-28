use std::cmp::Ordering;

use itertools::Itertools;

use crate::System;

impl PartialEq<Self> for dyn System + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

impl Eq for dyn System + '_ {}

impl PartialOrd<Self> for dyn System + '_ {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let components = self.component_accesses();
        let other_components = other.component_accesses();

        let overlapping_components: Vec<_> = components
            .into_iter()
            .cartesian_product(other_components.into_iter())
            .filter(|(a, b)| a.component_type() == b.component_type())
            .collect();

        if overlapping_components.is_empty() {
            return Some(Ordering::Equal);
        }

        let mutation_equal = overlapping_components
            .iter()
            .any(|(a, b)| a.is_write() == b.is_write());
        if mutation_equal {
            return Some(Ordering::Equal);
        }

        let other_writes = overlapping_components
            .iter()
            .any(|(a, b)| a.is_read() && b.is_write());
        if other_writes {
            return Some(Ordering::Greater);
        }

        let other_reads = overlapping_components
            .iter()
            .any(|(a, b)| a.is_write() && b.is_read());
        if other_reads {
            return Some(Ordering::Less);
        }

        None
    }
}

impl Ord for dyn System + '_ {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}

#[cfg(test)]
mod tests {
    use crate::{IntoSystem, Read, SystemParameter, Write};

    use super::*;

    #[derive(Debug, Default)]
    pub struct A(i32);
    #[derive(Debug, Default)]
    pub struct B(String);
    #[derive(Debug, Default)]
    pub struct C(f32);

    fn read_a_system(_: Read<A>) {}
    fn read_a_system1(_: Read<A>) {}
    fn write_a_system(_: Write<A>) {}
    fn write_a_system1(_: Write<A>) {}

    fn read_many_write_a(_: Read<B>, _: Write<A>) {}

    fn into_system<F: IntoSystem<Parameters>, Parameters: SystemParameter>(
        function: F,
    ) -> Box<dyn System> {
        Box::new(function.into_system())
    }

    #[test]
    fn system_writing_to_component_precedes_system_reading_from_component() {
        let read_system = into_system(read_a_system);
        let write_system = into_system(write_a_system);

        let ordering0 = read_system.partial_cmp(&write_system);
        let ordering1 = write_system.partial_cmp(&read_system);

        // read_system >= write_system
        assert_eq!(Some(Ordering::Greater), ordering0);
        // write_system <= read_system
        assert_eq!(Some(Ordering::Less), ordering1);
    }

    #[test]
    fn systems_writing_to_component_are_of_equal_precedence() {
        let write_system0 = into_system(write_a_system);
        let write_system1 = into_system(write_a_system1);

        let ordering0 = write_system0.partial_cmp(&write_system1);
        let ordering1 = write_system1.partial_cmp(&write_system0);

        assert_eq!(Some(Ordering::Equal), ordering0);
        assert_eq!(Some(Ordering::Equal), ordering1);
    }

    #[test]
    fn systems_reading_from_component_are_of_equal_precedence() {
        let read_system0 = into_system(read_a_system);
        let read_system1 = into_system(read_a_system1);

        let ordering0 = read_system0.partial_cmp(&read_system1);
        let ordering1 = read_system1.partial_cmp(&read_system0);

        assert_eq!(Some(Ordering::Equal), ordering0);
        assert_eq!(Some(Ordering::Equal), ordering1);
    }

    #[test]
    fn system_reading_from_many_and_writing_to_component_precedes_system_read_from_component() {
        let many_system = into_system(read_many_write_a);
        let read_system = into_system(read_a_system);

        let ordering0 = many_system.partial_cmp(&read_system);
        let ordering1 = read_system.partial_cmp(&many_system);

        assert_eq!(Some(Ordering::Less), ordering0);
        assert_eq!(Some(Ordering::Greater), ordering1);
    }
}
