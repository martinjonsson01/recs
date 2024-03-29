use ecs_bench_suite::{bevy, recs};

#[test]
fn debug_simple_iter_bench() {
    let mut bench = recs::simple_iter::Benchmark::new();
    bench.run();
}

#[test]
fn debug_frag_iter_bench() {
    let mut bench = recs::frag_iter::Benchmark::new();
    bench.run();
}

#[test]
fn debug_schedule_bench() {
    let mut bench = recs::schedule::Benchmark::new();
    bench.run();
}

#[test]
fn debug_heavy_compute_bench() {
    let mut bench = recs::heavy_compute::Benchmark::new();
    bench.run();
}

#[test]
fn debug_add_remove_bench() {
    let mut bench = recs::add_remove::Benchmark::new();
    bench.run();
}

#[test]
fn debug_n_body_bench() {
    let mut bench = recs::n_body::Benchmark::new(100);
    bench.run(100);
}

#[test]
fn debug_bevy_n_body_bench() {
    let mut bench = bevy::n_body::Benchmark::new(100);
    bench.run(100);
}

#[test]
fn debug_rain_simulation_bench() {
    let mut bench = recs::rain_simulation::Benchmark::new(100);
    bench.run(100);
}

#[test]
fn debug_bevy_rain_simulation_bench() {
    let mut bench = bevy::rain_simulation::Benchmark::new(100);
    bench.run(100);
}
