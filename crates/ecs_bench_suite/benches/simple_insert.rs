use criterion::*;
use ecs_bench_suite::*;

fn bench_simple_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_insert");
    #[cfg(feature = "bench-all-engines")]
    {
        group.bench_function("legion", |b| {
            let mut bench = legion::simple_insert::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("bevy", |b| {
            let mut bench = bevy::simple_insert::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("hecs", |b| {
            let mut bench = hecs::simple_insert::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("planck_ecs", |b| {
            let mut bench = planck_ecs::simple_insert::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("shipyard", |b| {
            let mut bench = shipyard::simple_insert::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("specs", |b| {
            let mut bench = specs::simple_insert::Benchmark::new();
            b.iter(move || bench.run());
        });
    }
    group.bench_function("recs", |b| {
        let mut bench = recs::simple_insert::Benchmark::new();
        b.iter(move || bench.run());
    });
    group.bench_function("naive_vec", |b| {
        let mut bench = naive::simple_insert::Benchmark::new();
        b.iter(move || bench.run());
    });
}

criterion_group!(simple_insert, bench_simple_insert,);
criterion_main!(simple_insert);
