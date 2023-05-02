use cgmath::{Deg, MetricSpace};
use crossbeam::channel::unbounded;
use ecs::systems::command_buffers::Commands;
use ecs::systems::{Query, Read};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder, Entity};
use gfx_plugin::Graphical;
use n_body::scenes::{
    create_rendered_planet_entity, create_rendered_sun_entity, SINGLE_HEAVY_BODY_AT_ORIGIN,
    SMALL_HEAVY_CLUSTERS,
};
use n_body::{acceleration, gravity, movement, BodySpawner, GenericResult, Position};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use tracing::instrument;

const COLLISION_RADIUS: f32 = 0.1;

#[instrument]
fn main() -> GenericResult<()> {
    let mut app_builder = BasicApplicationBuilder::default().with_rendering()?;

    #[cfg(feature = "profile")]
    {
        use ecs::profiling::Profileable;
        app_builder = app_builder.with_profiling()?;
    }
    #[cfg(not(feature = "profile"))]
    {
        use ecs::logging::Loggable;
        app_builder = app_builder.with_tracing()?;
    }

    let mut app = app_builder
        .output_directory(env!("OUT_DIR"))
        .light_model("sphere.obj")
        .field_of_view(Deg(90.0))
        .far_clipping_plane(10_000.0)
        .camera_movement_speed(100.0)
        .add_system(movement)
        .add_system(acceleration)
        .add_system(gravity)
        .add_system(collision)
        .build()?;

    SMALL_HEAVY_CLUSTERS.spawn_bodies(&mut app, create_rendered_planet_entity)?;
    SINGLE_HEAVY_BODY_AT_ORIGIN.spawn_bodies(&mut app, create_rendered_sun_entity)?;

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}

fn collision(
    entity: Entity,
    position: Read<Position>,
    others_positions: Query<(Entity, Read<Position>)>,
    commands: Commands,
) {
    for (other_entity, other_position) in others_positions {
        if other_entity == entity {
            continue;
        }

        let distance = position.point.distance2(other_position.point);

        if distance < (COLLISION_RADIUS * COLLISION_RADIUS) {
            commands.remove(entity);
            break;
        }
    }
}
