using Unity.Entities;
using static Constants;

public partial class AccelerationSystem : SystemBase
{
    protected override void OnUpdate()
    {
        new AccelerationJob().ScheduleParallel();
    }
}

[BurstCompile]
public partial struct AccelerationJob : IJobEntity
{
    private void Execute(ref Velocity velocity, in Acceleration acceleration)
    {
        velocity.Vector += acceleration.Vector * FIXED_TIME_STEP;
    }
}
