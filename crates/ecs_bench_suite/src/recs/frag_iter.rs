use ecs::systems::iteration::SequentiallyIterable;
use ecs::systems::{IntoSystem, Write};
use ecs::{Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder};
use std::sync::OnceLock;

macro_rules! create_entities {
    ($app:ident; $( $variants:ident ),*) => {
        $(
            #[derive(Debug)]
            struct $variants(f32);

            for _ in 0..20 {
                let entity = $app.create_entity().unwrap();
                $app.add_component(entity, $variants(0.0)).unwrap();
                $app.add_component(entity, Data(0.0)).unwrap();
            }
        )*
    };
}

#[derive(Debug)]
struct Data(f32);

static APPLICATION: OnceLock<BasicApplication> = OnceLock::new();

pub struct Benchmark;

impl Benchmark {
    pub fn new() -> Self {
        APPLICATION.get_or_init(|| {
            let mut app = BasicApplicationBuilder::default().build();

            create_entities!(app; A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);

            app
        });

        Self
    }

    pub fn run(&mut self) {
        let app = APPLICATION.get().unwrap();

        let doubling_system = |mut data: Write<Data>| {
            data.0 *= 2.0;
        };
        let doubling_system = doubling_system.into_system();

        doubling_system.run(&app.world).unwrap();
    }
}
