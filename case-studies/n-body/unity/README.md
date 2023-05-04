# Building
- Install Unity 2022.2.17f1 or later. Install the IL2CPP module if you will benchmark with this scripting backend.
- Open the `./N-body` folder as a project in Unity.
- You can change scripting backend from the menu `Edit > Project Settings > Player`, then choose Mono or IL2CPP under `Other Settings > Configuration > Scripting Backend`.
- Open `Build Settings` from `File > Build Settings...`.
- Make sure that only one scene is enabled in `Scenes In Build`. `Scenes/GameObject/Scene` for GameObjects or `Scenes/ECS/Scene` for ECS.
- Press `Build`

# Running
It is recommended to launch the benchmark using the `bench_n_body.py` script. More details in `recs/benchmarking/README.md`.

To run the benchmark manually, launch the executable with that arguments `-batchmode -nographics` to run it in headless mode. The benchmark result will be saved in a csv file in the working directory. It is also possible to launch the benchmark with graphics by excluding the arguments.

## Running Unity GameObjects in the Editor
- Open the scene `Scenes/GameObject/Scene`.
- Press `Play` (top middle) to start the simulation in the Editor.

## Running Unity ECS in the Editor
- Open the scene `Scenes/ECS/Scene`.
- Open the inspector for the SubScene and make sure the subscene is opened, otherwise press `Open` in the inspector.
- Press `Play` (top middle) to start the simulation in the Editor.
