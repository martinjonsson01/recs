use ecs::{Application, ApplicationBuilder, BasicApplication, BasicApplicationBuilder, Entity};

#[derive(Debug)]
struct A(f32);

#[derive(Debug)]
struct B(f32);

pub struct Benchmark(BasicApplication, Vec<Entity>);

impl Benchmark {
    pub fn new() -> Self {
        let mut app = BasicApplicationBuilder::default().build();

        let entities: Vec<_> = (0..10000)
            .map(|_| {
                let entity = app.create_empty_entity().unwrap();

                app.add_component(entity, A(0.0)).unwrap();

                entity
            })
            .collect();

        Self(app, entities)
    }

    pub fn run(&mut self) {
        let Benchmark(app, entities) = self;
        for &entity in &*entities {
            app.add_component(entity, B(0.0)).unwrap();
        }

        for &entity in &*entities {
            app.remove_component::<B>(entity).unwrap();
        }
    }
}
