use cgmath::Deg;
use color_eyre::Report;
use crossbeam::channel::unbounded;
use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
use gfx_plugin::Graphical;
use rain_simulation::scene::create_evenly_interspersed_clouds;
use rain_simulation::{
    gravity, ground_collision, movement, rain_visual, vapor_accumulation, wind, wind_direction,
    RAINDROP_MODEL,
};
use scheduler::executor::WorkerPool;
use scheduler::schedule::PrecedenceGraph;
use tracing::instrument;

#[instrument]
fn main() -> Result<(), Report> {
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
        .add_system(gravity)
        .add_system(wind)
        .add_system(wind_direction)
        .add_system(movement)
        .add_system(rain_visual)
        .add_system(ground_collision)
        .add_system(vapor_accumulation)
        .build()?;

    RAINDROP_MODEL
        .set(app.load_model("moon.obj").expect("moon.obj file exists"))
        .expect("model has not been initialized yet");

    let clouds_components = create_evenly_interspersed_clouds(1000);
    for cloud_components in clouds_components {
        app.create_entity(cloud_components)?;
    }

    #[cfg(feature = "asmr")]
    let song_thread = std::thread::spawn(|| {
        let player = playback_rs::Player::new(None).unwrap();
        let output_path = std::path::PathBuf::from(env!("OUT_DIR"));
        let assets_path = output_path.join(std::path::Path::new("assets"));
        let mp3_path = assets_path.join(std::path::Path::new("asmr.mp3"));
        let song = playback_rs::Song::from_file(mp3_path, Some(0.1)).unwrap();
        loop {
            player
                .play_song_now(&song, None)
                .expect("Song is very good and nice to listen to");
            while player.has_current_song() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    });

    let (_shutdown_sender, shutdown_receiver) = unbounded();
    app.run::<WorkerPool, PrecedenceGraph>(shutdown_receiver)?;

    #[cfg(feature = "asmr")]
    song_thread.join().expect("asmr would never panic");

    Ok(())
}
