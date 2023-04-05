//! The core managing module of the engine, responsible for high-level startup and error-handling.

use std::cell::Cell;
use std::error::Error;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam::channel::{unbounded, Receiver, RecvTimeoutError, SendError, Sender};
use derivative::Derivative;
pub use ring_channel::RingSender;
use ring_channel::{ring_channel, RingReceiver};
use thiserror::Error;
use tracing::{error, instrument, span, trace, warn, Level};
use winit::window::Window;

use crate::camera::CameraController;
use crate::renderer::{ModelHandle, Renderer, RendererError};
use crate::time::Time;
use crate::window::{InputEvent, Windowing, WindowingCommand, WindowingError, WindowingEvent};
use crate::{PointLight, Position, Transform};

/// An error that has occurred within the engine.
#[derive(Error, Debug)]
pub enum EngineError {
    /// Could not instantiate State.
    #[error("could not instantiate State")]
    StateConstruction(#[source] Box<RendererError>),
    /// Could not create a window.
    #[error("could not create a window")]
    WindowCreation(#[source] WindowingError),
    /// Failed to render frame.
    #[error("failed to render frame")]
    Rendering(#[source] Box<RendererError>),
    /// Could not load model.
    #[error("could not load model `{1}`")]
    ModelLoad(#[source] Box<RendererError>, PathBuf),
    /// Could not create a new object.
    #[error("a new object could not be created")]
    ObjectCreation(#[source] Box<RendererError>),
    /// Could not create a single new object.
    #[error("a single new object could not be created")]
    SingleObjectCreation(),
    /// The main thread has closed, causing the simulation thread to not be able to send it data.
    #[error("the main thread has closed")]
    MainThreadClosed(#[source] SendError<MainMessage>),
    /// An error occurred during graphics initialization by the client.
    #[error("an error occurred during graphics initialization by the client")]
    RenderInitialization(#[source] Box<dyn Error + Send + Sync>),
    /// A critical error has occurred and the application should shut down as soon as possible.
    #[error(
        "a critical error has occurred and the application should shut down as soon as possible"
    )]
    Critical(#[source] RendererError),
    /// Could not send command to windowing system.
    #[error("could not send command to windowing system")]
    SendWindowingCommand(#[source] SendError<WindowingCommand>),
    /// An error occurred during simulation tick.
    #[error("an error occurred during simulation tick")]
    SimulationThread(#[source] GenericError),
    /// Tried to shut down while the engine is already in the process of shutting down.
    #[error("tried to shut down while the engine is already in the process of shutting down")]
    AlreadyShuttingDown(),
}

/// Whether an engine operation failed or succeeded.
pub type EngineResult<T, E = EngineError> = Result<T, E>;

const AVERAGE_FPS_SAMPLES: usize = 128;

/// The driving actor of all windowing, rendering and simulation.
#[derive(Debug)]
pub struct GraphicsEngine<UIFn, RenderData, LightData> {
    main: MainThread<RenderData, LightData>,
    /// The window management wrapper.
    windowing: Windowing<UIFn, RenderData, LightData>,
    /// The current state of the renderer.
    renderer: Renderer<UIFn, RenderData, LightData>,
}

const CAMERA_SPEED: f32 = 7.0;
const CAMERA_SENSITIVITY: f32 = 1.0;

/// A generic error type that hides the type of the error by boxing it.
pub type GenericError = Box<dyn Error + Send + Sync>;

/// A generic result type that can be returned by client code, where it's not as important
/// what the exact type of the error is.
pub type GenericResult<T> = Result<T, GenericError>;

/// An internal message passed to the main thread at engine-level.
#[derive(Debug)]
pub enum MainMessage {
    /// Sent from simulation thread to inform about its speed.
    SimulationRate {
        /// The time that passed between one simulation tick and the next.
        delta_time: Duration,
    },
    /// Sent from simulation thread to inform about an error.
    SimulationError(GenericError),
}

/// Rendering of the user interface.
pub trait UIRenderer {
    /// Set up the user interface for the given context.
    fn render(context: &egui::Context);
}

/// Disables UI rendering.
#[derive(Debug)]
pub struct NoUI;
impl UIRenderer for NoUI {
    fn render(_: &egui::Context) {}
}

/// A way of communicating (both sending data to and receiving data from) with the graphics engine.
#[derive(Debug)]
pub struct EngineHandle<RenderData, LightData> {
    /// How the graphics engine receives information about what to render and where to render it.
    pub render_data_sender: RingSender<RenderData>,
    /// How the graphics engine receives information about which lights to render and where they are.
    pub light_data_sender: RingSender<LightData>,
    /// A receiver whose sender is dropped when the user has exited the application window.
    ///
    /// todo(#63): Change this so that the user code is responsible for handling the
    /// todo(#63): "shutdown event" and can choose to drop the shutdown sender themselves.'
    pub shutdown_receiver: Receiver<()>,
    /// Can be used by the simulation thread to inform the main thread about events.
    pub main_thread_sender: Sender<MainMessage>,
}

impl<UI, RenderData, LightData> GraphicsEngine<UI, RenderData, LightData>
where
    UI: UIRenderer + 'static,
    for<'a> RenderData: IntoIterator<Item = (ModelHandle, Vec<Transform>)> + Send + 'a,
    for<'a> LightData: IntoIterator<Item = (PointLight, Position)> + Default + Send + 'a,
{
    /// Creates a new instance of `Engine`.
    pub fn new() -> EngineResult<(Self, EngineHandle<RenderData, LightData>)> {
        let data_buffer_channel_capacity = NonZeroUsize::new(1).expect("1 is non-zero");
        let (render_data_sender, render_data_receiver) = ring_channel(data_buffer_channel_capacity);
        let (light_data_sender, light_data_receiver) = ring_channel(data_buffer_channel_capacity);

        let (windowing, window_event_receiver, window_command_sender) =
            Windowing::new().map_err(EngineError::WindowCreation)?;
        let renderer = Renderer::new(&windowing.window)
            .map_err(|e| EngineError::StateConstruction(Box::new(e)))?;

        let (main_thread_sender, main_thread_receiver) = unbounded();
        let (shutdown_sender, shutdown_receiver) = unbounded();

        let main = MainThread {
            time: Time::new(Instant::now(), AVERAGE_FPS_SAMPLES),
            camera_controller: CameraController::new(CAMERA_SPEED, CAMERA_SENSITIVITY),
            render_data_receiver,
            light_data_receiver,
            window_event_receiver,
            window_command_sender,
            shutdown_sender: Cell::new(Some(shutdown_sender)),
            main_thread_receiver,
        };

        Ok((
            Self {
                main,
                windowing,
                renderer,
            },
            EngineHandle {
                render_data_sender,
                light_data_sender,
                shutdown_receiver,
                main_thread_sender,
            },
        ))
    }

    /// Initializes and starts all state and threads, beginning the core event-loops of the program.
    pub fn start(self) -> EngineResult<()> {
        let GraphicsEngine {
            mut main,
            windowing,
            renderer,
        } = self;
        windowing.run(
            renderer,
            move |renderer, window, egui_context, egui_state| {
                let span = span!(Level::INFO, "main");
                let _enter = span.enter();

                main.tick(renderer)?;
                renderer.tick(window, egui_context, egui_state)
            },
        )
    }

    /// Gets the [`Creator`] associated with the graphics engine, which can then be used
    /// to instantiate objects.
    pub fn get_object_creator(&mut self) -> &mut impl Creator {
        &mut self.renderer
    }
}

/// The main thread of the engine, which runs the windowing event loop and render loop.
#[derive(Derivative)]
#[derivative(Debug)]
struct MainThread<RenderData, LightData> {
    /// The current time of the engine.
    time: Time,
    /// A controller for moving the camera around based on user input.
    camera_controller: CameraController,
    /// Used to receive data about what to render.
    render_data_receiver: RingReceiver<RenderData>,
    /// Used to receive data about lights.
    light_data_receiver: RingReceiver<LightData>,
    /// Used to listen to window events.
    window_event_receiver: Receiver<WindowingEvent>,
    /// Used to send commands to the windowing system.
    window_command_sender: Sender<WindowingCommand>,
    /// Used to signal when to shut down.
    #[derivative(Debug = "ignore")]
    shutdown_sender: Cell<Option<Sender<()>>>,
    /// Used to receive information from other threads.
    main_thread_receiver: Receiver<MainMessage>,
}

impl<RenderData, LightData> MainThread<RenderData, LightData>
where
    for<'a> RenderData: IntoIterator<Item = (ModelHandle, Vec<Transform>)> + Send + 'a,
    for<'a> LightData: IntoIterator<Item = (PointLight, Position)> + Default + Send + 'a,
{
    fn tick<UIFn>(
        &mut self,
        renderer: &mut Renderer<UIFn, RenderData, LightData>,
    ) -> EngineResult<()> {
        let span = span!(Level::INFO, "engine");
        let _enter = span.enter();

        let mut simulation_delta_samples = vec![];
        for event in self.main_thread_receiver.try_iter() {
            match event {
                MainMessage::SimulationRate { delta_time } => {
                    simulation_delta_samples.push(delta_time);
                }
                MainMessage::SimulationError(error) => {
                    self.signal_shutdown(Some(error))?;
                }
            }
        }
        self.time
            .simulation
            .update_from_delta_samples(simulation_delta_samples);

        for event in self.window_event_receiver.try_iter() {
            match event {
                WindowingEvent::Input(InputEvent::Close) => {
                    error!("got close event");
                    self.signal_shutdown(None)?;
                }
                WindowingEvent::Input(input) => {
                    self.camera_controller.input(&input);
                }
                WindowingEvent::Resized(new_size) => {
                    renderer.resize(new_size);
                }
            }
        }

        let render_rate = &self.time.render;
        trace!("gfx {render_rate}");

        let simulation_rate = &self.time.simulation;
        trace!("sim {simulation_rate}");

        if let Ok(render_data) = self.render_data_receiver.try_recv() {
            let light_data = self.light_data_receiver.try_recv().unwrap_or_default();

            self.time.render.update_time(Instant::now());

            // todo: render even if no data received by sim-thread
            renderer.update(
                &self.time.render,
                render_data,
                light_data,
                |camera, update_rate| {
                    self.camera_controller
                        .update_camera(camera, update_rate.delta_time);
                },
            );
        }

        Ok(())
    }

    #[instrument(skip_all, fields(simulation_error))]
    fn signal_shutdown(&self, simulation_error: Option<GenericError>) -> EngineResult<()> {
        self.shutdown_sender
            .take()
            .map(drop)
            .ok_or(EngineError::AlreadyShuttingDown())?;

        // Block until simulation thread has exited, before continuing with own shutdown,
        // because otherwise the exit of the main thread will kill the simulation thread.
        let timeout = Duration::from_secs(1);
        let result = loop {
            if let Err(e) = self.main_thread_receiver.recv_timeout(timeout) {
                break e;
            }
        };
        if let RecvTimeoutError::Timeout = result {
            warn!("simulation thread failed to exit within {timeout:?}, forcing exit...")
        }

        if let Some(error) = simulation_error {
            // Propagate error instead of sending quit window command, since that will
            // eventually quit as well, but will log the error.
            Err(EngineError::SimulationThread(error))
        } else {
            self.window_command_sender
                .send(WindowingCommand::Quit(0))
                .map_err(EngineError::SendWindowingCommand)
        }
    }
}

impl<UI, Data, LightData> Renderer<UI, Data, LightData>
where
    UI: UIRenderer + 'static,
{
    fn tick(
        &mut self,
        window: &Window,
        egui_context: &mut egui::Context,
        egui_state: &mut egui_winit::State,
    ) -> EngineResult<()> {
        let span = span!(Level::INFO, "render");
        let _enter = span.enter();

        match self.render(window, egui_state, egui_context) {
            Ok(_) => Ok(()),
            // Reconfigure the surface if lost.
            Err(RendererError::MissingOutputTexture(wgpu::SurfaceError::Lost)) => {
                // Resizing to same size effectively recreates the surface.
                self.resize(window.inner_size());
                Ok(())
            }
            // The system is out of memory, we should probably quit.
            Err(error)
                if matches!(
                    error,
                    RendererError::MissingOutputTexture(wgpu::SurfaceError::OutOfMemory)
                ) =>
            {
                Err(EngineError::Critical(error))
            }
            // `SurfaceError::Outdated` occurs when the app is minimized on Windows.
            // Silently return here to prevent spamming the console with "Outdated".
            Err(RendererError::MissingOutputTexture(wgpu::SurfaceError::Outdated)) => Ok(()),
            // All other surface errors (Timeout) should be resolved by the next frame.
            Err(RendererError::MissingOutputTexture(error)) => {
                error!("{error:?}");
                Ok(())
            }
            // Pass on any other rendering errors.
            Err(error) => Err(EngineError::Rendering(Box::new(error))),
        }
    }
}

/// A way of creating objects in the renderer.
pub trait Creator {
    /// Loads a model into the engine.
    ///
    /// # Examples
    /// ```no_run
    /// use gfx::engine::{Creator, EngineError, EngineResult, GenericResult};
    ///
    /// fn initialize_gfx(mut creator: impl Creator) -> EngineResult<()> {
    ///     let model_path = std::path::Path::new("path/to/model.obj");
    ///     let model_handle = creator.load_model(model_path)?;
    ///     Ok(())
    /// }
    /// ```
    fn load_model(&mut self, path: &Path) -> EngineResult<ModelHandle>;
}

impl<UIFn, Data, LightData> Creator for Renderer<UIFn, Data, LightData> {
    #[instrument(skip(self))]
    fn load_model(&mut self, path: &Path) -> EngineResult<ModelHandle> {
        self.load_model(path)
            .map_err(|e| EngineError::ModelLoad(Box::new(e), path.to_owned()))
    }
}
