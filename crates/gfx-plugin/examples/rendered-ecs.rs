use cgmath::{Deg, InnerSpace, Quaternion, Rotation3, Vector3, Zero};
use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::logging::Loggable;
use ecs::systems::{Read, Write};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use gfx::PointLight;
use gfx_plugin::rendering::{Position, Rotation, Scale};
use gfx_plugin::Graphical;
use rand::Rng;
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use tracing::instrument;

// a simple example of how to use the crate `ecs`
#[instrument]
fn main() -> Result<(), Report> {
    let mut app = BasicApplicationBuilder::default()
        .with_rendering()?
        .with_tracing()?
        .add_system(rotation_system)
        .add_system(light_animation_system)
        .build()?;

    let cube_model = app.load_model("cube.obj")?;

    let mut random = rand::thread_rng();
    for _ in 0..10 {
        let position = Position {
            vector: [
                random.gen_range(0_f32..10.0),
                random.gen_range(0_f32..10.0),
                random.gen_range(0_f32..10.0),
            ]
            .into(),
        };
        let scale = Scale {
            vector: [
                random.gen_range(0.1..1.0),
                random.gen_range(0.1..1.0),
                random.gen_range(0.1..1.0),
            ]
            .into(),
        };

        let _entity = app
            .rendered_entity_builder(cube_model)?
            .with_position(position)
            .with_scale(scale)
            .build()?;
    }

    let light = app.create_entity()?;
    app.add_component(light, PointLight::default())?;
    app.add_component(
        light,
        Position {
            vector: [10.0, 0.0, 0.0].into(),
        },
    )?;

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}
// todo(#90): Take into account for delta time (add delta time as a resource)
const ROTATION_DELTA: f32 = 1.0;

fn rotation_system(position: Read<Position>, mut rotation: Write<Rotation>) {
    let rotation_axis = if position.vector.is_zero() {
        Vector3::unit_z()
    } else {
        position.vector.normalize()
    };
    let rotate_around_axis = Quaternion::from_axis_angle(rotation_axis, Deg(ROTATION_DELTA));
    rotation.quaternion = rotation.quaternion * rotate_around_axis;
}

// todo(#90): Take into account for delta time (add delta time as a resource)
const DEGREES_PER_SECOND: f32 = 0.1;

fn light_animation_system(mut position: Write<Position>, _: Read<PointLight>) {
    let rotation = Quaternion::from_axis_angle(Vector3::unit_y(), Deg(DEGREES_PER_SECOND));
    position.vector = rotation * position.vector;
}
