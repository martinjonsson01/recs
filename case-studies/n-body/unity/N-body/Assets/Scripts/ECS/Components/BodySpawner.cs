using Unity.Entities;

public struct BodySpawner : IComponentData
{
    public Entity Moon;
    public Entity Sun;
    public int BodyCount;
}
