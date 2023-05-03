use ecs::systems::{IntoSystem, Query, Write};
use ecs::{Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder};
use std::sync::OnceLock;

macro_rules! create_entities {
    ($app:ident; $( $variants:ident ),*) => {
        $(
            #[derive(Debug)]
            struct $variants(f32);

            for _ in 0..20 {
                $app.create_entity(($variants(0.0),Data(0.0))).unwrap();
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

        let system = (|| {}).into_system();
        let query: Query<(Write<Data>,)> = Query::new(&app.world, &system);

        let query_iterator = query.try_into_iter().unwrap();
        for (mut data,) in query_iterator {
            data.0 *= 2.0;
        }
    }
}
