use ecs_bench_suite::recs;

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
