use ecs::systems::{Read, Write};
use ecs::{
    Application, ApplicationBuilder, BasicApplicationBuilder, IntoTickable, Sequential, Tickable,
    Unordered,
};
use ntest::timeout;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Mutex;
//noinspection RsUnusedImport -- For some reason CLion can't detect that it's being used.
use test_log::test;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct A;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct B(u32, i32);

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct C(i32);

#[test]
#[timeout(1000)]
fn system_is_passed_component_values_for_each_entity() {
    let expected_components = vec![A, A, A];
    let expected_component_count = expected_components.len();

    static READ_COMPONENTS: Mutex<Vec<A>> = Mutex::new(vec![]);
    let system = move |component: Read<A>| {
        let mut read_components = READ_COMPONENTS.lock().unwrap();
        read_components.push(*component);
    };

    let mut app = BasicApplicationBuilder::default()
        .add_system(system)
        .build();

    let entity = app.create_entity().unwrap();
    app.add_component(entity, A).unwrap();

    let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
    while READ_COMPONENTS.lock().unwrap().len() < expected_component_count {
        runner.tick().unwrap();
    }

    assert_eq!(expected_components, *READ_COMPONENTS.try_lock().unwrap());
}

#[test]
#[timeout(1000)]
fn system_mutates_component_values() {
    const NEW_VALUE: i32 = 100;
    let expected_components = vec![B(0, NEW_VALUE), B(1, NEW_VALUE), B(2, NEW_VALUE)];
    let expected_component_count = expected_components.len();

    static READ_COMPONENTS: Mutex<Vec<B>> = Mutex::new(vec![]);
    // Writes a new value to each component B
    let write_system = |mut component: Write<B>| {
        component.1 = NEW_VALUE;
    };

    // Reads all components of B and checks if they have been updated to the new value.
    let read_system = move |component: Read<B>| {
        let mut read_components = READ_COMPONENTS.lock().unwrap();
        if component.1 == NEW_VALUE {
            read_components.push(*component);
        }
    };

    let mut app = BasicApplicationBuilder::default()
        .add_system(write_system)
        .add_system(read_system)
        .build();

    for identifier in 0..expected_component_count {
        let entity = app.create_entity().unwrap();
        app.add_component(entity, B(identifier as u32, 0)).unwrap();
    }

    let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
    while READ_COMPONENTS.lock().unwrap().len() < expected_component_count {
        runner.tick().unwrap();
    }

    assert_eq!(expected_components, *READ_COMPONENTS.try_lock().unwrap());
}

#[test]
#[timeout(1000)]
fn multiparameter_systems_run_with_component_values_queried() {
    const ENTITY_COUNT: u8 = 10;

    static THREE_PARAMETER_COUNT: AtomicU8 = AtomicU8::new(0);
    static TWO_PARAMETER_COUNT: AtomicU8 = AtomicU8::new(0);
    static ONE_PARAMETER_COUNT: AtomicU8 = AtomicU8::new(0);

    let three_parameter_system = move |_: Read<A>, _: Read<B>, _: Write<C>| {
        THREE_PARAMETER_COUNT.fetch_add(1, SeqCst);
    };
    let two_parameter_system = move |_: Read<A>, _: Read<B>| {
        TWO_PARAMETER_COUNT.fetch_add(1, SeqCst);
    };
    let one_parameter_system = move |_: Read<A>| {
        ONE_PARAMETER_COUNT.fetch_add(1, SeqCst);
    };

    let mut app = BasicApplicationBuilder::default()
        .add_system(three_parameter_system)
        .add_system(two_parameter_system)
        .add_system(one_parameter_system)
        .build();

    let entity = app.create_entity().unwrap();
    app.add_component(entity, A).unwrap();

    for _ in 0..ENTITY_COUNT {
        let entity = app.create_entity().unwrap();
        app.add_component(entity, A).unwrap();
        app.add_component(entity, B(0, 0)).unwrap();
        app.add_component(entity, C(0)).unwrap();
    }

    let all_systems_have_run_for_each_entity = || {
        THREE_PARAMETER_COUNT.load(SeqCst) >= ENTITY_COUNT
            && TWO_PARAMETER_COUNT.load(SeqCst) >= ENTITY_COUNT
            && ONE_PARAMETER_COUNT.load(SeqCst) >= ENTITY_COUNT
    };

    let mut runner = app.into_tickable::<Sequential, Unordered>().unwrap();
    while !all_systems_have_run_for_each_entity() {
        runner.tick().unwrap();
    }
}
