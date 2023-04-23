using Unity.Entities;
using Unity.Transforms;
using Unity.Burst;
using static Constants;

public partial class MovementSystem : SystemBase
{
    protected override void OnUpdate()
    {
        new MovementJob().ScheduleParallel();
    }
}

[BurstCompile]
public partial struct MovementJob : IJobEntity
{
    private void Execute(ref LocalTransform transform, in Velocity velocity)
    {
        transform.Position += velocity.Vector * FIXED_TIME_STEP;
    }
}
