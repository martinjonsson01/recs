use crossbeam::atomic::AtomicCell;
use crossbeam::channel::unbounded;
use crossbeam::sync::Parker;
use ecs::pool::ThreadPool;
use ecs::scheduling::Unordered;
use ecs::{Application, Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct A(i32);

#[test]
fn reads_and_writes_of_same_component_do_not_execute_concurrently() {
    let read_start = Arc::new(AtomicCell::new(None));
    let read_start_ref = Arc::clone(&read_start);
    let read_end = Arc::new(AtomicCell::new(None));
    let read_end_ref = Arc::clone(&read_end);
    let read_system = move |_: Read<A>| {
        let start = Instant::now();
        thread::sleep(Duration::from_millis(100));
        let end = Instant::now();
        read_start_ref.store(Some(start));
        read_end_ref.store(Some(end));
    };

    let shutdown_parker = Parker::new();
    let shutdown_unparker = shutdown_parker.unparker().clone();
    let (shutdown_sender, shutdown_receiver) = unbounded();

    let shutdown_thread = thread::spawn(move || {
        shutdown_parker.park();
        drop(shutdown_sender);
    });

    let write_start = Arc::new(AtomicCell::new(None));
    let write_start_ref = Arc::clone(&write_start);
    let write_end = Arc::new(AtomicCell::new(None));
    let write_end_ref = Arc::clone(&write_end);
    let write_system = move |_: Write<A>| {
        let start = Instant::now();
        thread::sleep(Duration::from_millis(100));
        let end = Instant::now();
        write_start_ref.store(Some(start));
        write_end_ref.store(Some(end));
        shutdown_unparker.unpark();
    };

    let mut app = Application::default()
        .add_system(read_system)
        .add_system(write_system);
    let entity = app.new_entity();
    app.add_component_to_entity(entity, A(1));

    app.run::<ThreadPool, Unordered>(shutdown_receiver);
    shutdown_thread.join().unwrap();

    let component_read_start = read_start.take().unwrap();
    let component_read_end = read_end.take().unwrap();
    let component_write_start = write_start.take().unwrap();
    let component_write_end = write_end.take().unwrap();
    assert!(component_read_start < component_write_start);
    assert!(component_read_end < component_write_start);
    assert!(component_write_start > component_read_end);
    assert!(component_write_end > component_read_end);
}
