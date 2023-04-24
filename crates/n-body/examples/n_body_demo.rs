use cgmath::Deg;
use crossbeam::channel::unbounded;
use ecs::systems::{Read, Write};
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use gfx_plugin::{rendering, Graphical};
use n_body::scenes::{
    create_rendered_planet_entity, create_rendered_sun_entity, SINGLE_HEAVY_BODY_AT_ORIGIN,
    SMALL_HEAVY_CLUSTERS,
};
use n_body::{
    recs_acceleration, recs_gravity, recs_movement, BodySpawner, GenericResult, Position,
};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use tracing::instrument;

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
        .add_system(recs_movement)
        .add_system(recs_acceleration)
        .add_system(recs_gravity)
        .add_system(keep_positions_synchronized)
        .build()?;

    SMALL_HEAVY_CLUSTERS.spawn_bodies(&mut app, create_rendered_planet_entity)?;
    SINGLE_HEAVY_BODY_AT_ORIGIN.spawn_bodies(&mut app, create_rendered_sun_entity)?;

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    Ok(())
}

fn keep_positions_synchronized(
    logical_position: Read<Position>,
    mut graphical_position: Write<rendering::Position>,
) {
    graphical_position.point = logical_position.point;
}
