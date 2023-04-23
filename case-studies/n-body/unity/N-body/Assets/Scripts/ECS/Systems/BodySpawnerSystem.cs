using Unity.Entities;
using Unity.Transforms;
using Unity.Burst;
using Unity.Mathematics;

[BurstCompile]
public partial struct BodySpawnerSystem : ISystem, ISystemStartStop
{
    [BurstCompile]
    public void OnStartRunning(ref SystemState state)
    {
        foreach (RefRO<BodySpawner> spawner in SystemAPI.Query<RefRO<BodySpawner>>())
        {
            ProcessBodySpawner(ref state, spawner);
        }
    }

    public void OnStopRunning(ref SystemState state) { }

    private void ProcessBodySpawner(ref SystemState state, RefRO<BodySpawner> spawner)
    {
        Random rnd = new Random();
        rnd.InitState();

        Entity sun = state.EntityManager.Instantiate(spawner.ValueRO.Sun);
        state.EntityManager.SetComponentData(sun, new LocalTransform {
            Position = rnd.NextFloat3(-1f, 1f),
            Scale = 1
        });
        state.EntityManager.SetComponentData(sun, new Velocity());
        state.EntityManager.SetComponentData(sun, new Mass {Scalar = 5e16f});

        for (int i = 1; i < spawner.ValueRO.BodyCount; i++)
        {
            Entity entity = state.EntityManager.Instantiate(spawner.ValueRO.Moon);
            var radius = rnd.NextFloat();
            state.EntityManager.SetComponentData(entity, new LocalTransform {
                Position = rnd.NextFloat3(-5f, 5f),
                Scale = radius,
            });
            state.EntityManager.SetComponentData(entity, new Velocity { Vector = rnd.NextFloat3Direction() * 1000f});
            state.EntityManager.SetComponentData(entity, new Mass {Scalar = 1e6f * radius * radius * radius});
        }
    }
}
