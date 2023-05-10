use criterion::*;
use ecs_bench_suite::recs;

const MINIMUM_NUMBER_OF_TICKS: u32 = 100;

fn bench_rain_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("rain_simulation");

    let running_in_ci = option_env!("CI").is_some();

    if !running_in_ci {
        group.sample_size(10);
        group.sampling_mode(SamplingMode::Flat);
    }

    let exponents = if running_in_ci {
        // The CI machine is quite a bit slower, so only run it for a small number of bodies.
        10..=10
    } else {
        0..=14
    };
    for clouds in exponents.map(|exponent| 2_u32.pow(exponent)) {
        #[cfg(feature = "bench-all-engines")]
        {
            /*group.bench_with_input(BenchmarkId::new("bevy", bodies), &bodies, |b, &bodies| {
                let mut bench = ecs_bench_suite::bevy::rain_simulation::Benchmark::new(bodies);
                b.iter_custom(move |iterations| {
                    // Always average over at least n iterations, to ensure cache benefits are
                    // recorded, even for slow benchmarks.
                    bench.run(MINIMUM_NUMBER_OF_TICKS as u64 * iterations) / MINIMUM_NUMBER_OF_TICKS
                });
            });*/
        }
        group.bench_with_input(BenchmarkId::new("recs", clouds), &clouds, |b, &bodies| {
            let mut bench = recs::rain_simulation::Benchmark::new(bodies);
            b.iter_custom(move |iterations| {
                // Always average over at least n iterations, to ensure cache benefits are
                // recorded, even for slow benchmarks.
                bench.run(MINIMUM_NUMBER_OF_TICKS as u64 * iterations) / MINIMUM_NUMBER_OF_TICKS
            });
        });
    }

    group.finish();
}

criterion_group!(rain_simulation, bench_rain_simulation,);
criterion_main!(rain_simulation);
