use bevy_ecs::{prelude::*, schedule::Schedule};
use n_body::scenes::{create_bevy_planet_entity, ALL_HEAVY_RANDOM_CUBE};
use n_body::{bevy_acceleration, bevy_gravity, bevy_movement, BodySpawner};

pub struct Benchmark(World, Schedule);

impl Benchmark {
    pub fn new() -> Self {
        let mut world = World::default();

        ALL_HEAVY_RANDOM_CUBE
            .spawn_bodies(&mut world, create_bevy_planet_entity)
            .unwrap();

        let mut schedule = Schedule::default();
        schedule.add_system(bevy_movement);
        schedule.add_system(bevy_acceleration);
        schedule.add_system(bevy_gravity);

        Self(world, schedule)
    }

    pub fn run(&mut self) {
        self.1.run(&mut self.0);
    }
}
