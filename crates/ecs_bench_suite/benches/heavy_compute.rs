use criterion::*;
use ecs_bench_suite::*;

fn bench_heavy_compute(c: &mut Criterion) {
    let mut group = c.benchmark_group("heavy_compute");
    #[cfg(feature = "bench-all-engines")]
    {
        group.bench_function("legion", |b| {
            let mut bench = legion::heavy_compute::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("legion (packed)", |b| {
            let mut bench = legion_packed::heavy_compute::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("bevy", |b| {
            let mut bench = bevy::heavy_compute::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("hecs", |b| {
            let mut bench = hecs::heavy_compute::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("shipyard", |b| {
            let mut bench = shipyard::heavy_compute::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("specs", |b| {
            let mut bench = specs::heavy_compute::Benchmark::new();
            b.iter(move || bench.run());
        });
    }
    group.bench_function("recs", |b| {
        let mut bench = recs::heavy_compute::Benchmark::new();
        b.iter(move || bench.run());
    });
    group.bench_function("naive_vec", |b| {
        let mut bench = naive::heavy_compute::Benchmark::new();
        b.iter(move || bench.run());
    });
}

criterion_group!(heavy_compute, bench_heavy_compute,);
criterion_main!(heavy_compute);
