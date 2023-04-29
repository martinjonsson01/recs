using Unity.Entities;
using Unity.Mathematics;
using Unity.Transforms;

public partial class BodySpawnerSystem : SystemBase
{
    public bool SpawnedBodies;

    protected override void OnUpdate()
    {
        if (SpawnedBodies) return;

        foreach (var spawner in SystemAPI.Query<RefRO<BodySpawner>>()) ProcessBodySpawner(spawner);

        SpawnedBodies = true;
    }

    private void ProcessBodySpawner(RefRO<BodySpawner> spawner)
    {
        var rnd = new Random();
        rnd.InitState();

        var sun = EntityManager.Instantiate(spawner.ValueRO.Sun);
        EntityManager.SetComponentData(sun, new LocalTransform
        {
            Position = rnd.NextFloat3(-1f, 1f),
            Scale = 1
        });
        EntityManager.SetComponentData(sun, new Velocity());
        EntityManager.SetComponentData(sun, new Mass { Scalar = 5e16f });

        for (var i = 1; i < BenchmarkSystem.CurrentBenchmark.BodyCount; i++)
        {
            var entity = EntityManager.Instantiate(spawner.ValueRO.Moon);
            var radius = rnd.NextFloat();
            EntityManager.SetComponentData(entity, new LocalTransform
            {
                Position = rnd.NextFloat3(-5f, 5f),
                Scale = radius
            });
            EntityManager.SetComponentData(entity, new Velocity { Vector = rnd.NextFloat3Direction() * 1000f });
            EntityManager.SetComponentData(entity, new Mass { Scalar = 1e6f * radius * radius * radius });
        }
    }
}
