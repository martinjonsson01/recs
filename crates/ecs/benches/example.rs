use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::thread;
use std::time::Duration;

fn sleep_100_microseconds(c: &mut Criterion) {
    c.bench_function("sleep", |bencher| {
        bencher.iter(|| thread::sleep(Duration::from_micros(100)))
    });
}

fn sleep_x_microseconds(c: &mut Criterion) {
    let mut group = c.benchmark_group("sleep x");
    for microseconds in 1..10 {
        group.throughput(Throughput::Elements(microseconds));
        group.bench_with_input(
            BenchmarkId::from_parameter(microseconds),
            &microseconds,
            |b, &microseconds| {
                b.iter(|| thread::sleep(Duration::from_micros(microseconds)));
            },
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = sleep_100_microseconds, sleep_x_microseconds
}
criterion_main!(benches);
