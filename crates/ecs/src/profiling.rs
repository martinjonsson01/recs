//! An add-on to `ecs::Application` that enables profiling using the
//! [Tracy profiling tool](https://github.com/wolfpld/tracy).

use crate::Application;
use tracing::metadata::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;

impl Application {
    /// Enables profiling connection to Tracy.
    #[allow(clippy::print_stdout)] // Because we can't print to console using tracing when it's disabled
    pub fn with_profiling(self) -> Self {
        tracing_subscriber::registry()
            .with(tracing_tracy::TracyLayer::new().with_filter(LevelFilter::INFO))
            .init();

        println!("running with profiling enabled!");
        println!("this means console output will be limited in favor of performance");
        self
    }
}
