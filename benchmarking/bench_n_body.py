import os
from pathlib import Path
import time
import csv
import platform
import subprocess

start_time = time.perf_counter()

print("running Rust benchmarks...")
os.chdir("..")
# os.system("cargo bench --bench n_body --features bench-all-engines")

end_time = time.perf_counter()

bench_duration = end_time - start_time
print(f"benchmarking is done! Took {bench_duration / 60:0.2f} minutes")


class BenchmarkResults:
    def __init__(self):
        self.bench_name = "None"
        self.body_count = -1
        self.ticks_per_sample = -1
        self.samples = []

    def __str__(self):
        return f"{self.bench_name} - bodies: {self.body_count}, " \
               f"ticks per sample: {self.ticks_per_sample}, " \
               f"samples: {self.samples}"

    def write_as_row(self, csv_writer):
        csv_writer.writerow([self.body_count, self.ticks_per_sample] + localize_floats(self.samples))


def localize_floats(row):
    return [
        str(element).replace('.', ',') if isinstance(element, float) else element
        for element in row
    ]


def read_results_from(path):
    if not path.exists():
        print(f"error: can't read benchmark results from {path}")
        return None

    results = BenchmarkResults()
    with path.open("r") as file:
        reader = csv.reader(file, delimiter=',')

        next(reader)

        for row in reader:
            results.bench_name = f"{row[0]}_{row[1]}"
            results.body_count = int(row[2])

            iteration_count = int(row[7])

            sample_duration_per_iteration = float(row[5]) / iteration_count
            if row[6] == "ns":
                sample_duration_per_iteration /= 1e9
            results.samples.append(sample_duration_per_iteration)

            # Each Rust-bench runs 100x more ticks than Criterion requests
            results.ticks_per_sample = iteration_count * 100

    return results


benchmarking_directory = Path(__file__).parent
n_body_results_directory = benchmarking_directory.parent / "target" / "criterion" / "n_body"


def collect_engine_results(engine_name):
    results_directory = n_body_results_directory / engine_name
    if not results_directory.exists():
        print(f"error: can't find benchmark directory {results_directory}")
        return None

    body_size_directories = [file for file in results_directory.iterdir() if file.is_dir()]
    engine_results = []
    for body_size_directory in body_size_directories:
        if body_size_directory.name == "report":
            continue

        bench_results = read_results_from(body_size_directory / "new" / "raw.csv")
        if bench_results is not None:
            engine_results.append(bench_results)

    return sorted(engine_results, key=lambda result: result.body_count)


print("collecting results... ", end='')
bevy_results = collect_engine_results("bevy")
recs_results = collect_engine_results("recs")
print("done")


def write_results_to_csv(engine_results):
    if engine_results is None or len(engine_results) == 0:
        print("error: can't write empty results")
        return

    results_file = benchmarking_directory / f"{engine_results[0].bench_name}.csv"
    with results_file.open("w", newline="") as file:
        writer = csv.writer(file)

        writer.writerow(["body_count", "ticks_per_sample"] + [f"sample_{n}" for n in range(0, 10)])

        for result in engine_results:
            result.write_as_row(writer)


print("saving results to csv... ", end='')
write_results_to_csv(bevy_results)
write_results_to_csv(recs_results)
print("done")

print(f"results placed in directory {benchmarking_directory}")


def open_file(path):
    if platform.system() == "Windows":
        os.startfile(path)
    elif platform.system() == "Darwin":
        subprocess.Popen(["open", path])
    else:
        subprocess.Popen(["xdg-open", path])


open_file(benchmarking_directory)
