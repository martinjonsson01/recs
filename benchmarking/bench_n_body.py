import os
import shutil
from pathlib import Path
import time
import csv
import platform
import subprocess


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
        format(element, ".15f").replace('.', ',') if isinstance(element, float) else element
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


def is_executable(file: Path) -> bool:
    ext = os.path.splitext(file)[1]
    return not file.is_dir() and (ext == ".exe" or ext == ".x86_64" or (ext == "" and os.access(file, os.X_OK)))


def run_precompiled_benchmarks():
    precompiled_benchmarks_directory = benchmarking_directory / "precompiled-benchmarks"
    if not precompiled_benchmarks_directory.exists():
        print(f"warning: {precompiled_benchmarks_directory} does not exist. skipping precompiled benchmarks.")
        return
    os.chdir(precompiled_benchmarks_directory)
    precompiled_benchmark_directories = [file for file in precompiled_benchmarks_directory.iterdir() if file.is_dir()]

    for precompiled_benchmark in precompiled_benchmark_directories:
        os.chdir(precompiled_benchmark)

        files = precompiled_benchmark.iterdir()
        is_unity = next(precompiled_benchmark.glob("UnityPlayer.*"), False)

        binary = next((file for file in files if is_executable(file)), None)
        if binary is None:
            print(f"error: could not find a binary in {precompiled_benchmark}.")
            pass

        print(f"running {binary.name}...")

        cmd = [binary]
        if is_unity:
            cmd += ["-batchmode", "-nographics"]

        proc = subprocess.Popen(cmd, shell=False)
        proc.wait()

        csv = next(precompiled_benchmark.glob("*.csv"))
        if not csv.exists():
            print(f"error: could not find a csv for {binary}.")
            pass
        shutil.copyfile(csv, benchmarking_directory / f"{precompiled_benchmark.name}.csv")


def open_file(path):
    if platform.system() == "Windows":
        os.startfile(path)
    elif platform.system() == "Darwin":
        subprocess.Popen(["open", path], stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT)
    else:
        subprocess.Popen(["xdg-open", path], stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT)


if __name__ == '__main__':
    start_time = time.perf_counter()

    print("running Rust benchmarks...")
    os.chdir("..")
    os.system("cargo bench --bench n_body --features bench-all-engines")

    print("running precompiled benchmarks...")
    run_precompiled_benchmarks()

    end_time = time.perf_counter()

    bench_duration = end_time - start_time
    print(f"benchmarking is done! Took {bench_duration / 60:0.2f} minutes")

    print("collecting results... ", end='')
    bevy_results = collect_engine_results("bevy")
    recs_results = collect_engine_results("recs")
    print("done")

    print("saving results to csv... ", end='')
    write_results_to_csv(bevy_results)
    write_results_to_csv(recs_results)
    print("done")

    print(f"results placed in directory {benchmarking_directory}")
    open_file(benchmarking_directory)
