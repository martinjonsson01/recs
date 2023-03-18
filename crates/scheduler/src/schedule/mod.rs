//! Schedules that produce orderings of systems that are correct.
//!
//! Correct, meaning they (try to) guarantee:
//! * freedom from system starvation, meaning all systems get to execute in the schedule ordering;
//! * freedom from race conditions, meaning the ordering will not place reads and writes to
//!   the same component at the same time;
//! * freedom from deadlock, meaning systems that are ordered such that they will always
//!   be able to progress.

use daggy::Dag;
use ecs::System;
use std::fmt::{Debug, Formatter};

type Sys<'system> = &'system dyn System;

/// Orders [`System`]s based on their precedence.
#[derive(Default, Clone)]
pub struct PrecedenceGraph<'systems> {
    dag: Dag<Sys<'systems>, ()>,
}

impl<'systems> Debug for PrecedenceGraph<'systems> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Instead of printing the struct, format the DAG in dot-format and print that.
        // (so it can be viewed through tools like http://viz-js.com/)
        writeln!(f, "{:?}", daggy::petgraph::dot::Dot::new(self.dag.graph()))
    }
}

// PrecedenceGraph-equality should be based on whether the underlying DAGs are
// isomorphic or not - i.e. whether they're equivalently constructed but not necessarily
// exactly the same.
impl<'systems> PartialEq<Self> for PrecedenceGraph<'systems> {
    fn eq(&self, other: &Self) -> bool {
        let node_match = |a: &Sys<'systems>, b: &Sys<'systems>| a == b;
        let edge_match = |_: &(), _: &()| true;
        daggy::petgraph::algo::is_isomorphic_matching(
            &self.dag.graph(),
            &other.dag.graph(),
            node_match,
            edge_match,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daggy::petgraph::dot::Dot;
    use ecs::{IntoSystem, Read, SystemParameters, Write};
    use test_log::test;

    #[derive(Debug, Default)]
    pub struct A(i32);
    #[derive(Debug, Default)]
    pub struct B(&'static str);
    #[derive(Debug, Default)]
    pub struct C(f32);

    fn read_a(_: Read<A>) {}
    fn read_b(_: Read<B>) {}
    fn write_a(_: Write<A>) {}

    fn into_system<F: IntoSystem<Parameters>, Parameters: SystemParameters>(
        function: F,
    ) -> Box<dyn System> {
        Box::new(function.into_system())
    }

    #[test]
    fn precedence_graphs_are_equal_if_isomorphic() {
        let read_a = into_system(read_a);
        let write_a = into_system(write_a);
        let read_b = into_system(read_b);

        let mut a = PrecedenceGraph::default();
        let a_node0 = a.dag.add_node(read_a.as_ref());
        let a_node1 = a.dag.add_node(write_a.as_ref());
        let a_node2 = a.dag.add_node(read_b.as_ref());
        a.dag
            .add_edges([(a_node0, a_node1, ()), (a_node0, a_node2, ())])
            .unwrap();

        let mut b = PrecedenceGraph::default();
        let b_node0 = b.dag.add_node(read_a.as_ref());
        let b_node1 = b.dag.add_node(write_a.as_ref());
        let b_node2 = b.dag.add_node(read_b.as_ref());
        // Same edges as a but added in reverse order.
        b.dag
            .add_edges([(b_node0, b_node2, ()), (b_node0, b_node1, ())])
            .unwrap();

        // When comparing the raw contents (i.e. exact same order of edges and same indices of nodes)
        // they should not be equal, since edges were added in different orders.
        let a_dot = format!("{:?}", Dot::new(a.dag.graph()));
        let b_dot = format!("{:?}", Dot::new(b.dag.graph()));
        assert_ne!(a_dot, b_dot);

        // But they are isomorphic.
        assert_eq!(a, b);
    }
}
