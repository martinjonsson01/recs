use crossbeam::channel::unbounded;
use ecs::{Application, Read, Sequential, Unordered, Write};
use ntest::timeout;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};

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

    let (shutdown_sender, shutdown_receiver) = unbounded();
    let read_components = Arc::new(Mutex::new(vec![]));
    let read_components_ref = Arc::clone(&read_components);
    let system = move |component: Read<A>| {
        let mut read_components = read_components_ref.lock().unwrap();
        read_components.push(*component);
        if read_components.len() == expected_component_count {
            shutdown_sender.send(()).unwrap();
        }
    };

    let mut app = Application::default().add_system(system);

    let entity = app.create_entity();
    app.add_component(entity, A);

    app.run::<Sequential, Unordered>(shutdown_receiver);

    assert_eq!(expected_components, *read_components.try_lock().unwrap());
}

#[test]
#[timeout(1000)]
fn system_mutates_component_values() {
    const NEW_VALUE: i32 = 100;
    let expected_components = vec![B(0, NEW_VALUE), B(1, NEW_VALUE), B(2, NEW_VALUE)];
    let expected_component_count = expected_components.len();

    let (shutdown_sender, shutdown_receiver) = unbounded();
    let read_components = Arc::new(Mutex::new(vec![]));
    let read_components_ref = Arc::clone(&read_components);
    // Writes a new value to each component B
    let write_system = |mut component: Write<B>| {
        component.1 = NEW_VALUE;
    };

    // Reads all components of B and checks if they have been updated to the new value.
    let read_system = move |component: Read<B>| {
        let mut read_components = read_components_ref.lock().unwrap();
        if component.1 == NEW_VALUE {
            read_components.push(*component);
        }
        if read_components.len() == expected_component_count {
            shutdown_sender.send(()).unwrap();
        }
    };

    let mut app = Application::default()
        .add_system(write_system)
        .add_system(read_system);

    for identifier in 0..expected_component_count {
        let entity = app.create_entity();
        app.add_component(entity, B(identifier as u32, 0));
    }

    app.run::<Sequential, Unordered>(shutdown_receiver);

    assert_eq!(expected_components, *read_components.try_lock().unwrap());
}

#[test]
#[timeout(1000)]
fn multiparameter_systems_run_with_component_values_queried() {
    const ENTITY_COUNT: u8 = 10;

    let (shutdown_sender, shutdown_receiver) = unbounded();
    let three_parameter_count = Arc::new(AtomicU8::new(0));
    let two_parameter_count = Arc::new(AtomicU8::new(0));
    let one_parameter_count = Arc::new(AtomicU8::new(0));
    let three_parameter_count_ref = Arc::clone(&three_parameter_count);
    let two_parameter_count_ref = Arc::clone(&two_parameter_count);
    let one_parameter_count_ref = Arc::clone(&one_parameter_count);
    // Makes sure all systems have run once for each entity, otherwise test times out
    let shutdown_thread = std::thread::spawn(move || loop {
        if three_parameter_count_ref.load(SeqCst) == ENTITY_COUNT
            && two_parameter_count_ref.load(SeqCst) == ENTITY_COUNT
            && one_parameter_count_ref.load(SeqCst) == ENTITY_COUNT
        {
            shutdown_sender.send(()).unwrap();
            break;
        }
    });

    let three_parameter_system = move |_: Read<A>, _: Read<B>, _: Write<C>| {
        three_parameter_count.fetch_add(1, SeqCst);
    };
    let two_parameter_system = move |_: Read<A>, _: Read<B>| {
        two_parameter_count.fetch_add(1, SeqCst);
    };
    let one_parameter_system = move |_: Read<A>| {
        one_parameter_count.fetch_add(1, SeqCst);
    };

    let mut app = Application::default()
        .add_system(three_parameter_system)
        .add_system(two_parameter_system)
        .add_system(one_parameter_system);

    let entity = app.create_entity();
    app.add_component(entity, A);

    for _ in 0..ENTITY_COUNT {
        let entity = app.create_entity();
        app.add_component(entity, A);
        app.add_component(entity, B(0, 0));
        app.add_component(entity, C(0));
    }

    app.run::<Sequential, Unordered>(shutdown_receiver);
    shutdown_thread.join().unwrap();
}
