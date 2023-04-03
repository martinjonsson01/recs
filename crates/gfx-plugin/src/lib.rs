//! A rendering-plugin for RECS, which renders the [`BasicApplication`] using a renderer
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

use crossbeam::channel::Receiver;
use ecs::systems::{IntoSystem, SystemParameters};
use ecs::{Application, Entity, Executor, Schedule};
use gfx::engine::{EngineError, NoUI};
use gfx::Object;
use std::error::Error;
use std::fmt::Debug;
use std::thread;
use thiserror::Error;

/// A decorator for [`Application`] which adds rendering functionality.
#[derive(Debug)]
pub struct GraphicalApplication<App> {
    application: App,
}

// Delegate all methods that are the same as `BasicApplication`.
impl<App> Application for GraphicalApplication<App>
where
    App: Application + Send,
{
    type Error = GraphicalApplicationError;

    #[inline(always)]
    fn create_entity(&mut self) -> Result<Entity, Self::Error> {
        self.application
            .create_entity()
            .map_err(|error| GraphicalApplicationError::InternalApplication(Box::new(error)))
    }

    #[inline(always)]
    fn add_system<System, Parameters>(mut self, system: System) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.application = self.application.add_system(system);
        self
    }

    #[inline(always)]
    fn add_systems<System, Parameters>(mut self, systems: impl IntoIterator<Item = System>) -> Self
    where
        System: IntoSystem<Parameters>,
        Parameters: SystemParameters,
    {
        self.application = self.application.add_systems(systems);
        self
    }

    #[inline(always)]
    fn add_component<ComponentType: Debug + Send + Sync + 'static>(
        &mut self,
        entity: Entity,
        component: ComponentType,
    ) -> Result<(), Self::Error> {
        self.application
            .add_component(entity, component)
            .map_err(|error| GraphicalApplicationError::InternalApplication(Box::new(error)))
    }

    fn run<'systems, E: Executor<'systems>, S: Schedule<'systems>>(
        &'systems mut self,
        _shutdown_receiver: Receiver<()>,
    ) -> Result<(), Self::Error> {
        let (graphics_engine, graphics_engine_handle): (GraphicsEngine, GraphicsEngineHandle) =
            gfx::engine::Engine::new()
                .map_err(GraphicalApplicationError::GraphicsEngineInitialization)?;

        thread::scope(|scope| {
            thread::Builder::new()
                .name("simulation".to_string())
                .spawn_scoped(scope, move || {
                    #[allow(unused)] // todo(#87): use render_data_sender and main_thread_sender
                    let GraphicsEngineHandle {
                        render_data_sender,
                        shutdown_receiver,
                        main_thread_sender,
                    } = graphics_engine_handle;
                    self.application.run::<E, S>(shutdown_receiver)
                })
                .expect("there are no null bytes in the name");

            graphics_engine
                .start()
                .map_err(GraphicalApplicationError::GraphicsEngineStart)
        })
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
}

/// Whether an operation on the graphical application succeeded.
pub type GfxAppResult<T, E = GraphicalApplicationError> = Result<T, E>;

/// Enables rendering functionality.
pub trait Graphical<RenderedApp: Application> {
    /// Initializes and configures rendering functionality, enabling visualization of
    /// the simulation.
    fn with_rendering(self) -> GfxAppResult<RenderedApp>;
}

type GraphicsEngine = gfx::engine::Engine<NoUI, Vec<Object>>;
type GraphicsEngineHandle = gfx::engine::EngineHandle<Vec<Object>>;

impl<App: Application + Send> Graphical<GraphicalApplication<App>> for App {
    fn with_rendering(self) -> GfxAppResult<GraphicalApplication<App>> {
        Ok(GraphicalApplication { application: self })
    }
}
