//! An add-on to `ecs::Application` that enables profiling using the
//! [Tracy profiling tool](https://github.com/wolfpld/tracy).

use crate::profiling::ProfilingError::GlobalSubscriber;
use crate::ApplicationBuilder;
use thiserror::Error;
use tracing::metadata::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;

/// An error that occurred when setting up profiling.
#[derive(Error, Debug)]
pub enum ProfilingError {
    /// Failed to set up profiling trace subscriber.
    #[error("failed to set up profiling trace subscriber")]
    GlobalSubscriber(#[source] tracing_subscriber::util::TryInitError),
}

/// Whether a profiling operation succeeded.
pub type ProfilingResult<T, E = ProfilingError> = Result<T, E>;

/// Represents an [`ApplicationBuilder`] which can build an [`crate::Application`] which can be profiled.
pub trait Profileable: Sized {
    /// Enables profiling connection to Tracy.
    fn with_profiling(self) -> ProfilingResult<Self>;
}

impl<AppBuilder: ApplicationBuilder> Profileable for AppBuilder {
    #[allow(clippy::print_stdout)] // Because we can't print to console using tracing when it's disabled
    fn with_profiling(self) -> ProfilingResult<Self> {
        tracing_subscriber::registry()
            .with(tracing_tracy::TracyLayer::new().with_filter(LevelFilter::TRACE))
            .try_init()
            .map_err(GlobalSubscriber)?;

        println!("running with profiling enabled!");
        println!("this means console output will be limited in favor of performance");
        Ok(self)
    }
}
