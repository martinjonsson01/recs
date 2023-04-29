using Unity.Entities;

internal class BodyAuthoring : MonoBehaviour
{
    public float mass;
    public Vector3 velocity;
}

internal class BodyBaker : Baker<BodyAuthoring>
{
    public override void Bake(BodyAuthoring authoring)
    {
        var entity = GetEntity(TransformUsageFlags.None);
        AddComponent(entity, new Velocity { Vector = authoring.velocity });
        AddComponent(entity, new Acceleration());
        AddComponent(entity, new Mass { Scalar = authoring.mass });
    }
}
