use criterion::*;
use ecs_bench_suite::{bevy, recs};

fn bench_n_body(c: &mut Criterion) {
    let mut group = c.benchmark_group("n_body");

    for bodies in (0..10_000).step_by(1_000) {
        group.bench_with_input(BenchmarkId::new("recs", bodies), &bodies, |b, &bodies| {
            let mut bench = recs::n_body::Benchmark::new(bodies);
            b.iter(move || bench.run());
        });
        group.bench_with_input(BenchmarkId::new("bevy", bodies), &bodies, |b, &bodies| {
            let mut bench = bevy::n_body::Benchmark::new(bodies);
            b.iter(move || bench.run());
        });
    }
}

criterion_group!(n_body, bench_n_body,);
criterion_main!(n_body);
