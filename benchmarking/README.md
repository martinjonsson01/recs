# Benchmarking RECS

## N-body

### Prerequisites
* Python 3.4 or later
* Rust nightly 1.71 or later (+ cargo)

### How to run
```cmd
python bench_n_body.py
```

Output (`.csv`) files are located in this directory (`recs/benchmarking`).


## Precompiled benchmarks
The `bench_n_body.py` script will also run all precompiled benchmarks
located in the `recs/benchmarking/precompiled-benchmarks`.
Each benchmark should be in a subdirectory in the `precompiled-benchmarks` directory.
The names of the subdirectories will be used as the names of the csv outputs.

### Example file structure
* recs
  * benchmarking
    * precompiled-benchmarks
      * entt-msvc
        * n_body.exe
      * unity-gameobjects-mono
        * n_body_Data
        * n_body.exe
        * UnityPlayer.dll
