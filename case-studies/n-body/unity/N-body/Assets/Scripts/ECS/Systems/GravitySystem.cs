using Unity.Entities;
using Unity.Mathematics;
using Unity.Transforms;
using static Constants;

public partial class GravitySystem : SystemBase
{
    private EntityQuery query;

    protected override void OnCreate()
    {
        query = GetEntityQuery(ComponentType.ReadOnly<LocalTransform>(), ComponentType.ReadOnly<Mass>());
    }

    [BurstCompile]
    protected override void OnUpdate()
    {
        var transforms = query.ToComponentDataArray<LocalTransform>(Allocator.TempJob);
        var masses = query.ToComponentDataArray<Mass>(Allocator.TempJob);

        var job = new GravityJob
        {
            BodyCount = query.CalculateEntityCount(),
            Transforms = transforms,
            Masses = masses
        };

        job.ScheduleParallel();
        Dependency.Complete();

        transforms.Dispose();
        masses.Dispose();
    }
}

[BurstCompile]
public partial struct GravityJob : IJobEntity
{
    [ReadOnly] public int BodyCount;
    [ReadOnly] public NativeArray<LocalTransform> Transforms;
    [ReadOnly] public NativeArray<Mass> Masses;

    private void Execute(in LocalTransform transform, ref Acceleration acceleration)
    {
        float3 netAcceleration = 0;

        for (var i = 0; i < BodyCount; i++)
        {
            var toBody = Transforms[i].Position - transform.Position;
            var distanceSquared = math.lengthsq(toBody);

            if (distanceSquared < math.EPSILON) continue;

            var newAcceleration = GRAVITATIONAL_CONSTANT * Masses[i].Scalar / distanceSquared;
            netAcceleration += math.normalize(toBody) * newAcceleration;
        }

        acceleration.Vector = netAcceleration;
    }
}
