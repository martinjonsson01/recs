use cgmath::{One, Quaternion};
use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::logging::Loggable;
use ecs::systems::Write;
use ecs::{Application, BasicApplication};
use gfx::Transform;
use gfx_plugin::Graphical;
use rand::Rng;
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use std::thread;
use std::time::Duration;
use tracing::{instrument, warn};

// a simple example of how to use the crate `ecs`
#[instrument]
fn main() -> Result<(), Report> {
    let mut app = BasicApplication::default()
        .with_rendering()?
        .with_tracing()?
        .add_system(movement_system);

    let mut random = rand::thread_rng();
    for _ in 0..10 {
        let position = [
            random.gen_range(0_f32..10.0),
            random.gen_range(0_f32..10.0),
            random.gen_range(0_f32..10.0),
        ]
        .into();
        let scale = [
            random.gen_range(0.1..1.0),
            random.gen_range(0.1..1.0),
            random.gen_range(0.1..1.0),
        ]
        .into();

        let placement = Placement(Transform {
            position,
            rotation: Quaternion::one(),
            scale,
        });

        let entity = app.create_entity()?;
        app.add_component(entity, placement)?;
    }

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}

// todo(#87): replace with actual Rotation, Position and Scale components
#[derive(Debug)]
struct Placement(Transform);

#[instrument]
fn movement_system(mut a: Write<Placement>) {
    a.0.position.x += 0.001;
    thread::sleep(Duration::from_millis(10));
    warn!("i work! {a:#?}");
}