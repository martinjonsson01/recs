use ecs::System;
use test_strategy::proptest;
use test_utils::arb_system;

#[proptest]
fn systems_fulfill_equivalence_relation(
    #[strategy(arb_system())] a: Box<dyn System>,
    #[strategy(arb_system())] b: Box<dyn System>,
    #[strategy(arb_system())] c: Box<dyn System>,
) {
    reltester::eq(&a, &b, &c).unwrap()
}
