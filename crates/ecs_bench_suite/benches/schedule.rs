use criterion::*;
use ecs_bench_suite::*;

fn bench_schedule(c: &mut Criterion) {
    let mut group = c.benchmark_group("schedule");
    #[cfg(feature = "bench-all-engines")]
    {
        group.bench_function("legion", |b| {
            let mut bench = legion::schedule::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("legion (packed)", |b| {
            let mut bench = legion_packed::schedule::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("bevy", |b| {
            let mut bench = bevy::schedule::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("planck_ecs", |b| {
            let mut bench = planck_ecs::schedule::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("shipyard", |b| {
            let mut bench = shipyard::schedule::Benchmark::new();
            b.iter(move || bench.run());
        });
        group.bench_function("specs", |b| {
            let mut bench = specs::schedule::Benchmark::new();
            b.iter(move || bench.run());
        });
    }
    group.bench_function("recs", |b| {
        let mut bench = recs::schedule::Benchmark::new();
        b.iter(move || bench.run());
    });
}

criterion_group!(schedule, bench_schedule,);
criterion_main!(schedule);
