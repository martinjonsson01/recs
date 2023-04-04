//! A rendering-plugin for RECS, which renders the [`BasicApplication`](ecs::BasicApplication) using a renderer
//! built on [wgpu](https://github.com/gfx-rs/wgpu).

// rustc lints
#![warn(
    let_underscore,
    nonstandard_style,
    unused,
    explicit_outlives_requirements,
    meta_variable_misuse,
    missing_debug_implementations,
    missing_docs,
    non_ascii_idents,
    noop_method_call,
    pointer_structural_match,
    trivial_casts,
    trivial_numeric_casts
)]
// clippy lints
#![warn(
    clippy::cognitive_complexity,
    clippy::dbg_macro,
    clippy::if_then_some_else_none,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::rc_mutex,
    clippy::unwrap_used,
    clippy::large_enum_variant
)]

pub mod rendering;

use crate::rendering::{rendering_system, Model, RenderData, RenderQuery};
use crossbeam::channel::Receiver;
use ecs::systems::{IntoSystem, SystemParameters};
use ecs::{Application, ApplicationBuilder, Entity, Executor, Schedule};
use gfx::engine::Creator;
use gfx::engine::{EngineError, MainMessage, NoUI};
use gfx::time::UpdateRate;
use std::error::Error;
use std::fmt::Debug;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Instant;
use thiserror::Error;

/// Builds a [`GraphicalApplication`].
#[derive(Debug, Default)]
pub struct GraphicalApplicationBuilder<AppBuilder> {
    builder: AppBuilder,
}

impl<InnerApp, AppBuilder> ApplicationBuilder for GraphicalApplicationBuilder<AppBuilder>
where
    InnerApp: Application + Send,
    AppBuilder: ApplicationBuilder<App = InnerApp>,
{
    type App = GraphicsAppResult<GraphicalApplication<InnerApp>>;

    fn add_system<System, Parameters>(mut self, system: System) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.builder = self.builder.add_system(system);
        self
    }

    fn add_systems<System, Parameters>(mut self, systems: impl IntoIterator<Item = System>) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.builder = self.builder.add_systems(systems);
        self
    }

    fn build(self) -> Self::App {
        let (graphics_engine, graphics_engine_handle) = GraphicsEngine::new()
            .map_err(GraphicalApplicationError::GraphicsEngineInitialization)?;
        Ok(GraphicalApplication {
            application: self.builder.build(),
            graphics_engine: Some(graphics_engine),
            graphics_engine_handle: Some(graphics_engine_handle),
        })
    }
}

/// A decorator for [`Application`] which adds rendering functionality.
#[derive(Debug)]
pub struct GraphicalApplication<App> {
    application: App,
    graphics_engine: Option<GraphicsEngine>,
    graphics_engine_handle: Option<GraphicsEngineHandle>,
}

// Delegate all methods that are the same as `BasicApplication`.
impl<App> Application for GraphicalApplication<App>
where
    App: Application + Send + Sync,
{
    type Error = GraphicalApplicationError;

    #[inline(always)]
    fn create_entity(&mut self) -> Result<Entity, Self::Error> {
        self.application
            .create_entity()
            .map_err(to_internal_app_error)
    }

    #[inline(always)]
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> Result<(), Self::Error> {
        self.application
            .add_component(entity, component)
            .map_err(to_internal_app_error)
    }

    #[inline(always)]
    fn add_system<System, Parameters>(&mut self, system: System)
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.application.add_system(system);
    }

    fn run<'systems, E: Executor<'systems>, S: Schedule<'systems>>(
        &'systems mut self,
        _shutdown_receiver: Receiver<()>,
    ) -> Result<(), Self::Error> {
        thread::scope(|scope| {
            let application = &mut self.application;

            let graphics_engine = self
                .graphics_engine
                .take()
                .ok_or(GraphicalApplicationError::AlreadyStarted)?;
            let graphics_engine_handle = self
                .graphics_engine_handle
                .take()
                .ok_or(GraphicalApplicationError::AlreadyStarted)?;

            thread::Builder::new()
                .name("simulation".to_string())
                .spawn_scoped(scope, move || {
                    let GraphicsEngineHandle {
                        render_data_sender,
                        shutdown_receiver,
                        main_thread_sender,
                    } = graphics_engine_handle;

                    // todo(#90): replace with non-closure system which takes as input
                    // todo(#90): a resource `UpdateRate` and `MainThreadSender` instead of
                    // todo(#90): using a static and capturing variables like the below implementation.
                    let tick_rate_system = move || {
                        const AVERAGE_BUFFER_SIZE: usize = 128;
                        static UPDATE_RATE: Mutex<Option<UpdateRate>> = Mutex::new(None);

                        let mut update_rate =
                            UPDATE_RATE.lock().expect("lock should not be poisoned");

                        if let Some(update_rate) = &mut *update_rate {
                            update_rate.update_time(Instant::now());
                            main_thread_sender
                                .send(MainMessage::SimulationRate {
                                    delta_time: update_rate.delta_time,
                                })
                                .map_err(EngineError::MainThreadClosed)
                                .expect("main thread should be alive");
                        } else {
                            *update_rate =
                                Some(UpdateRate::new(Instant::now(), AVERAGE_BUFFER_SIZE));
                        }
                    };

                    // todo(#90): remove wrapper
                    let rendering_system_wrapper = move |query: RenderQuery| {
                        let render_data_sender = render_data_sender.clone();
                        rendering_system(render_data_sender, query);
                    };

                    application.add_system(tick_rate_system);
                    application.add_system(rendering_system_wrapper);
                    application.run::<E, S>(shutdown_receiver)
                })
                .expect("there are no null bytes in the name");

            graphics_engine
                .start()
                .map_err(GraphicalApplicationError::GraphicsEngineStart)
        })
    }
}

fn to_internal_app_error<E: Error + Send + Sync + 'static>(error: E) -> GraphicalApplicationError {
    GraphicalApplicationError::InternalApplication(Box::new(error))
}

impl<InnerApp: Application> GraphicalApplication<InnerApp> {
    /// Loads a model into the application.
    ///
    /// The returned [`Model`] is [`Clone`], meaning you can use the same model for multiple entities.
    ///
    /// # Examples
    /// ```no_run
    /// # use ecs::{Application, ApplicationBuilder, BasicApplicationBuilder};
    /// # use gfx_plugin::{Graphical, GraphicalApplicationError};
    /// # let mut app = BasicApplicationBuilder::default().with_rendering()?.build()?;
    ///
    /// let model_path = "path/to/model.obj";
    /// let model_component = app.load_model(model_path)?;
    ///
    /// let entity = app.create_entity()?;
    /// app.add_component(entity, model_component)?;
    ///
    /// Ok::<(), GraphicalApplicationError>(())
    /// ```
    pub fn load_model(&mut self, path_str: &str) -> GraphicsAppResult<Model> {
        let graphics_engine = self
            .graphics_engine
            .as_mut()
            .ok_or(GraphicalApplicationError::MissingGraphicsEngine)?;
        let path = Path::new(path_str);
        let handle = graphics_engine
            .get_object_creator()
            .load_model(path)
            .map_err(GraphicalApplicationError::ModelLoad)?;
        Ok(Model { handle })
    }
}

/// An error in the graphical application.
#[derive(Error, Debug)]
pub enum GraphicalApplicationError {
    /// Failed to initialize the graphics engine.
    #[error("failed to initialize the graphics engine")]
    GraphicsEngineInitialization(#[source] EngineError),
    /// Failed to start the graphics engine.
    #[error("failed to start the graphics engine")]
    GraphicsEngineStart(#[source] EngineError),
    /// An internal application error has occurred.
    #[error("an internal application error has occurred")]
    InternalApplication(#[source] Box<dyn Error + Send + Sync>),
    /// The application has already been started, can't start it again.
    #[error("the application has already been started, can't start it again")]
    AlreadyStarted,
    /// The application has already been started, so the graphics engine is not available.
    #[error("the application has already been started, so the graphics engine is not available")]
    MissingGraphicsEngine,
    /// Failed to load model into graphical application.
    #[error("failed to load model into graphical application")]
    ModelLoad(#[source] EngineError),
}

/// Whether an operation on the graphical application succeeded.
pub type GraphicsAppResult<T, E = GraphicalApplicationError> = Result<T, E>;

/// Enables rendering functionality.
pub trait Graphical<RenderedAppBuilder: ApplicationBuilder> {
    /// Initializes and configures rendering functionality, enabling visualization of
    /// the simulation.
    fn with_rendering(self) -> GraphicsAppResult<RenderedAppBuilder>;
}

type GraphicsEngine = gfx::engine::GraphicsEngine<NoUI, RenderData>;
type GraphicsEngineHandle = gfx::engine::EngineHandle<RenderData>;

impl<InnerApp, AppBuilder> Graphical<GraphicalApplicationBuilder<AppBuilder>> for AppBuilder
where
    InnerApp: Application + Send,
    AppBuilder: ApplicationBuilder<App = InnerApp>,
{
    fn with_rendering(self) -> GraphicsAppResult<GraphicalApplicationBuilder<AppBuilder>> {
        Ok(GraphicalApplicationBuilder { builder: self })
    }
}
