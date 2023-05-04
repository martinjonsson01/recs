use criterion::*;
use ecs_bench_suite::*;

fn bench_frag_iter_bc(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragmented_iter");
    #[cfg(feature = "bench-all-engines")]
    {
        group.bench_function("legion", |b| {
            let mut bench = legion::frag_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("bevy", |b| {
            let mut bench = bevy::frag_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("hecs", |b| {
            let mut bench = hecs::frag_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("planck_ecs", |b| {
            let mut bench = planck_ecs::frag_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("shipyard", |b| {
            let mut bench = shipyard::frag_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("specs", |b| {
            let mut bench = specs::frag_iter::Benchmark::new();
            b.iter(move || bench.run());
        });
    }
    group.bench_function("recs", |b| {
        let mut bench = recs::frag_iter::Benchmark::new();
        b.iter(move || bench.run());
    });
}

criterion_group!(frag_iter, bench_frag_iter_bc,);
criterion_main!(frag_iter);
