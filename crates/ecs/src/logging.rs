//! An add-on to `ecs::Application` that provides sophisticated and configurable
//! logging using `tracing`.

use crate::logging::LoggingError::{ColorInitialization, Configuration};
use crate::Application;
use thiserror::Error;
use time::format_description::well_known::Iso8601;
use time::UtcOffset;
use tracing_subscriber::fmt::time::OffsetTime;

/// An error that occurred when setting up logging.
#[derive(Error, Debug)]
pub enum LoggingError {
    /// Could not initialize coloring of logs.
    #[error("could not initialize coloring of logs")]
    ColorInitialization(#[source] color_eyre::Report),
    /// Failed to load logging configuration.
    #[error("failed to load logging configuration")]
    Configuration(#[source] tracing_subscriber::filter::ParseError),
}

/// Whether a logging operation succeeded.
pub type LoggingResult<T, E = LoggingError> = Result<T, E>;

impl Application {
    /// Attaches and initializes tracing infrastructure.
    pub fn with_tracing(self) -> LoggingResult<Self> {
        install_tracing()?;
        color_eyre::install().map_err(ColorInitialization)?;
        Ok(self)
    }
}

fn install_tracing() -> LoggingResult<()> {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    // In some environments it's not possible to get current local time zone offset without
    // invoking undefined behavior, so it might fail -- in which case we just use UTC.
    let offset = match UtcOffset::current_local_offset() {
        Ok(offset) => offset,
        Err(_) => UtcOffset::UTC,
    };

    let fmt_layer = fmt::layer()
        .with_thread_ids(true)
        .with_timer(OffsetTime::new(offset, Iso8601::DEFAULT))
        .with_target(false);
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("warn")) // Default to only warnings.
        .map_err(Configuration)?;

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();

    Ok(())
}
