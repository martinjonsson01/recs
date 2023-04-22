use criterion::*;

mod recs_n_body;

fn bench_bevy_vs_recs(c: &mut Criterion) {
    let mut group = c.benchmark_group("add_remove_component");

    group.bench_function("recs", |b| {
        let mut bench = recs_n_body::Benchmark::default();
        b.iter(move || bench.run());
    });
    /*group.bench_function("naive_vec", |b| {
        let mut bench = naive::add_remove::Benchmark::new();
        b.iter(move || bench.run());
    });*/
}

criterion_group!(bevy_vs_recs, bench_bevy_vs_recs,);
criterion_main!(bevy_vs_recs);
