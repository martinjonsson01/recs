use criterion::*;
use ecs_bench_suite::*;

fn bench_add_remove(c: &mut Criterion) {
    let mut group = c.benchmark_group("add_remove_component");
    #[cfg(feature = "bench-all-engines")]
    {
        group.bench_function("legion", |b| {
            let mut bench = legion::add_remove::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("hecs", |b| {
            let mut bench = hecs::add_remove::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("planck_ecs", |b| {
            let mut bench = planck_ecs::add_remove::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("shipyard", |b| {
            let mut bench = shipyard::add_remove::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("specs", |b| {
            let mut bench = specs::add_remove::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("bevy", |b| {
            let mut bench = bevy::add_remove::Benchmark::new();
            b.iter(move || bench.run());
        });
    }
    group.bench_function("recs", |b| {
        let mut bench = recs::add_remove::Benchmark::new();
        b.iter(move || bench.run());
    });
    group.bench_function("naive_vec", |b| {
        let mut bench = naive::add_remove::Benchmark::new();
        b.iter(move || bench.run());
    });
}

criterion_group!(add_remove, bench_add_remove,);
criterion_main!(add_remove);
