use criterion::*;
use ecs_bench_suite::recs;

fn bench_n_body(c: &mut Criterion) {
    let mut group = c.benchmark_group("n_body");

    group.bench_function("recs", |b| {
        let mut bench = recs::n_body::Benchmark::default();
        b.iter(move || bench.run());
    });
}

criterion_group!(n_body, bench_n_body,);
criterion_main!(n_body);
