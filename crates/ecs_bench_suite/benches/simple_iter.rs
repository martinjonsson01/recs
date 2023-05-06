use criterion::*;
use ecs_bench_suite::*;

fn bench_simple_iter(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_iter");
    #[cfg(feature = "bench-all-engines")]
    {
        group.bench_function("legion", |b| {
            let mut bench = legion::simple_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("legion (packed)", |b| {
            let mut bench = legion_packed::simple_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("bevy", |b| {
            let mut bench = bevy::simple_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("hecs", |b| {
            let mut bench = hecs::simple_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("planck_ecs", |b| {
            let mut bench = planck_ecs::simple_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("shipyard", |b| {
            let mut bench = shipyard::simple_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("specs", |b| {
            let mut bench = specs::simple_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
    }
    group.bench_function("recs", |b| {
        let mut bench = recs::simple_iter::Benchmark::new();
        b.iter(move || bench.run());
    });
    group.bench_function("naive_vec", |b| {
        let mut bench = naive::simple_iter::Benchmark::new();
        b.iter(move || bench.run());
    });
}

criterion_group!(simple_iter, bench_simple_iter,);
criterion_main!(simple_iter);
