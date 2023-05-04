using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using Unity.Collections;
using Unity.Entities;
using UnityEngine;
using UnityEngine.Rendering;
using UnityEngine.SceneManagement;
using static Constants;
using Debug = UnityEngine.Debug;
using Object = UnityEngine.Object;

public class Benchmark
{
    public readonly int BodyCount;

    public readonly TimeSpan[] Samples = new TimeSpan[SAMPLE_COUNT];
    public int TicksPerSample;

    public Benchmark(int bodyCount, int simulatedTicks)
    {
        BodyCount = bodyCount;
        TicksPerSample = simulatedTicks / SAMPLE_COUNT;
    }
}

public partial class BenchmarkSystem : SystemBase
{
    public static Benchmark CurrentBenchmark;
    private static Benchmark[] benchmarks;

    private readonly Stopwatch stopwatch = new();

    private int currentBenchmarkIndex;
    private int currentSample;

    private int currentTick = -1;

    private bool IsEcs => SceneManager.GetActiveScene().path.Contains("ECS");

    protected override void OnCreate()
    {
        benchmarks = new Benchmark[LARGEST_EXPONENT + 1];

        for (var n = 0; n <= LARGEST_EXPONENT; n++) benchmarks[n] = new Benchmark(1 << n, 1000);

        if (!IsEcs)
        {
            for (var n = 8; n <= LARGEST_EXPONENT; n++) benchmarks[n].TicksPerSample /= 10;
            for (var n = 12; n <= LARGEST_EXPONENT; n++) benchmarks[n].TicksPerSample /= 10;
        }

        CurrentBenchmark = benchmarks[0];
    }

    protected override void OnUpdate()
    {
        if (currentTick == 0) stopwatch.Start();
        if (currentTick == CurrentBenchmark.TicksPerSample)
        {
            stopwatch.Stop();

            CurrentBenchmark.Samples[currentSample] = stopwatch.Elapsed;

            if (++currentSample == SAMPLE_COUNT)
            {
                Debug.Log(
                    $"Finished benchmark with {CurrentBenchmark.BodyCount} {(CurrentBenchmark.BodyCount > 1 ? "bodies" : "body")}.");

                if (++currentBenchmarkIndex >= benchmarks.Length)
                {
                    Debug.Log("All benchmarks are done!");

                    SaveAsCsv();

                    if (SystemInfo.graphicsDeviceType == GraphicsDeviceType.Null) Application.Quit();
                }
                else
                {
                    CurrentBenchmark = benchmarks[currentBenchmarkIndex];

                    ResetSimulation();

                    currentSample = 0;
                    currentTick = -2;
                    stopwatch.Reset();
                }
            }
            else
            {
                currentTick = 0;
                stopwatch.Restart();
            }
        }

        currentTick++;
    }

    private void ResetSimulation()
    {
        if (IsEcs) ResetEcsSimulation();
        else ResetGameObjectsSimulation();
    }

    private void ResetEcsSimulation()
    {
        var ecb = new EntityCommandBuffer(Allocator.TempJob);
        Entities.ForEach((Entity entity, in Acceleration _) => ecb.DestroyEntity(entity)).Schedule();
        Dependency.Complete();
        ecb.Playback(EntityManager);
        ecb.Dispose();

        World.GetExistingSystemManaged<BodySpawnerSystem>().SpawnedBodies = false;
    }

    private void ResetGameObjectsSimulation()
    {
        foreach (var body in Object.FindObjectsOfType<Body>()) Object.Destroy(body.gameObject);

        Object.FindObjectOfType<GameObjectBodySpawner>().SpawnBodies();
    }

    private void SaveAsCsv()
    {
        var path = Path.GetFullPath($"unity_{(IsEcs ? "ecs" : "gameobjects")}.csv");
        Debug.Log($"Writing benchmark results to {path}.");

        using var writer = new StreamWriter(path);

        writer.Write("body_count,ticks_per_sample");
        for (var i = 0; i < SAMPLE_COUNT; i++) writer.Write($",sample_{i}");
        writer.WriteLine();

        foreach (var benchmark in benchmarks)
        {
            writer.Write($"{benchmark.BodyCount},{benchmark.TicksPerSample}");
            foreach (var sample in benchmark.Samples)
            {
                writer.Write(",\"");
                writer.Write((sample.TotalSeconds / benchmark.TicksPerSample).ToString("F15",
                    CultureInfo.GetCultureInfo("sv-SE")));
                writer.Write('"');
            }

            writer.WriteLine();
        }
    }
}
