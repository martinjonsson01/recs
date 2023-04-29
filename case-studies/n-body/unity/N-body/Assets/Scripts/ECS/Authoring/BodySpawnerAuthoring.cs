using Unity.Entities;

internal class BodySpawnerAuthoring : MonoBehaviour
{
    public GameObject moon;
    public GameObject sun;
}

internal class BodySpawnerBaker : Baker<BodySpawnerAuthoring>
{
    public override void Bake(BodySpawnerAuthoring authoring)
    {
        var entity = GetEntity(TransformUsageFlags.None);
        AddComponent(entity, new BodySpawner
        {
            Moon = GetEntity(authoring.moon, TransformUsageFlags.Dynamic),
            Sun = GetEntity(authoring.sun, TransformUsageFlags.Dynamic)
        });
    }
}
