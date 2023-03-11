//! An add-on to `ecs::Application` that provides sophisticated and configurable
//! logging using `tracing`.

use crate::Application;

impl Application {
    /// Attaches and initializes tracing infrastructure.
    pub fn with_tracing(self) -> Self {
        install_tracing();
        color_eyre::install().expect("todo: error handling");
        self
    }
}

fn install_tracing() {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let fmt_layer = fmt::layer().with_thread_ids(true).with_target(false);
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("warn"))
        .expect("todo: error handling");

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();
}
