using System.Diagnostics;
using Unity.Entities;
using Unity.Mathematics;
using UnityEngine;
using UnityEngine.Rendering;
using static Constants;
using Debug = UnityEngine.Debug;

public partial class BenchmarkSystem : SystemBase
{
    private readonly Stopwatch stopwatch = new();
    private int ticks = -1;

    protected override void OnUpdate()
    {
        if (ticks == 0)
        {
            stopwatch.Start();
        }
        else if (ticks == SIMULATED_TICKS)
        {
            stopwatch.Stop();
            var duration = stopwatch.Elapsed;
            double tps = SIMULATED_TICKS / duration.TotalSeconds;

            Debug.Log($"Benchmark took {math.round(duration.TotalMilliseconds)} ms to complete");
            Debug.Log($"{(float) tps} avg tps over {SIMULATED_TICKS} ticks");

            if (SystemInfo.graphicsDeviceType == GraphicsDeviceType.Null)
            {
                Application.Quit();
            }
        }

        ticks++;
    }
}
