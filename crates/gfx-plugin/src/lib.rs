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
use ecs::{Application, BasicApplication, BasicApplicationError, Entity, Executor, Schedule};
use gfx::engine::{
    Creator, EngineError, GenericResult, GraphicsInitializer, NoUI, RingSender, Simulation,
};
use gfx::time::UpdateRate;
use gfx::Object;
use std::fmt::Debug;
use thiserror::Error;

/// A decorator for [`BasicApplication`] which adds rendering functionality.
#[derive(Debug)]
pub struct GraphicalApplication {
    application: BasicApplication,
}

// Delegate all methods that are the same as `BasicApplication`.
impl Application for GraphicalApplication {
    type AppResult<T> = GfxAppResult<T>;

    #[inline(always)]
    fn create_entity(&mut self) -> Self::AppResult<Entity> {
        Ok(self.application.create_entity()?)
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
    ) -> Self::AppResult<()> {
        Ok(self.application.add_component(entity, component)?)
    }

    fn run<'systems, E: Executor<'systems>, S: Schedule<'systems>>(
        &'systems mut self,
        _shutdown_receiver: Receiver<()>,
    ) -> Self::AppResult<()> {
        let graphics_engine: GraphicsEngine = gfx::engine::Engine::new(())
            .map_err(GraphicalApplicationError::GraphicsEngineInitialization)?;
        graphics_engine
            .start()
            .map_err(GraphicalApplicationError::GraphicsEngineStart)
    }
}

/// Automatically converts any
/// [`BasicApplicationError`]s to [`GraphicalApplicationError::InternalApplication`].
impl From<BasicApplicationError> for GraphicalApplicationError {
    fn from(basic_error: BasicApplicationError) -> Self {
        GraphicalApplicationError::InternalApplication(basic_error)
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
    InternalApplication(#[source] BasicApplicationError),
}

/// Whether an operation on the graphical application succeeded.
pub type GfxAppResult<T, E = GraphicalApplicationError> = Result<T, E>;

/// Enables rendering functionality.
pub trait Graphical<RenderedApp: Application> {
    /// Initializes and configures rendering functionality, enabling visualization of
    /// the simulation.
    fn with_rendering(self) -> GfxAppResult<RenderedApp>;
}

#[derive(Debug)]
struct ECSRenderer;

impl GraphicsInitializer for ECSRenderer {
    type Context = ();

    fn initialize_graphics(
        _context: &mut Self::Context,
        _creator: &mut dyn Creator,
    ) -> GenericResult<()> {
        // todo(#87): implement
        Ok(())
    }
}

#[derive(Debug)]
struct ECSSimulation;

impl Simulation for ECSSimulation {
    type Context = ();
    type RenderData = Vec<Object>;

    fn tick(
        _context: &mut Self::Context,
        _time: &UpdateRate,
        _visualizations_sender: &mut RingSender<Self::RenderData>,
    ) -> GenericResult<()> {
        // todo(#87): implement
        Ok(())
    }
}

type GraphicsEngine = gfx::engine::Engine<ECSRenderer, NoUI, ECSSimulation, (), Vec<Object>>;

impl Graphical<GraphicalApplication> for BasicApplication {
    fn with_rendering(self) -> GfxAppResult<GraphicalApplication> {
        Ok(GraphicalApplication { application: self })
    }
}
