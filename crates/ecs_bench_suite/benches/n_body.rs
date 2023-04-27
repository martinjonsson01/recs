use criterion::*;
use ecs_bench_suite::recs;

fn bench_n_body(c: &mut Criterion) {
    let mut group = c.benchmark_group("n_body");

    for bodies in [1000] {
        group.bench_with_input(
            BenchmarkId::new("recs (old method)", bodies),
            &bodies,
            |b, &bodies| {
                let mut bench = recs::n_body_old::Benchmark::new(bodies);
                b.iter_custom(move |iterations| bench.run(iterations));
            },
        );
        #[cfg(feature = "bench-all-engines")]
        {
            group.bench_with_input(BenchmarkId::new("bevy", bodies), &bodies, |b, &bodies| {
                let mut bench = ecs_bench_suite::bevy::n_body::Benchmark::new(bodies);
                b.iter_custom(move |iterations| bench.run(iterations));
            });
        }
        group.bench_with_input(BenchmarkId::new("recs", bodies), &bodies, |b, &bodies| {
            let mut bench = recs::n_body::Benchmark::new(bodies);
            b.iter_custom(move |iterations| bench.run(iterations));
        });
    }
}

criterion_group!(n_body, bench_n_body,);
criterion_main!(n_body);
