using System.Collections.Generic;
using UnityEngine;

public class GameObjectBodySpawner : MonoBehaviour
{
    public GameObject moon;
    public GameObject sun;
    private readonly List<Body> bodies = new();

    private void Start()
    {
        SpawnBodies();
    }

    private void Update()
    {
        foreach (var body in bodies) body.UpdatePosition(bodies);
    }

    public void SpawnBodies()
    {
        bodies.Clear();

        var sunBody = Instantiate(sun, new Vector3(Random.Range(-1f, 1f), Random.Range(-1f, 1f), Random.Range(-1f, 1f)),
            Quaternion.identity);
        var sunBodyComponent = sunBody.AddComponent<Body>();
        sunBodyComponent.mass = 5e16f;
        bodies.Add(sunBodyComponent);

        for (var i = 1; i < BenchmarkSystem.CurrentBenchmark.BodyCount; i++)
        {
            var body = Instantiate(moon,
                new Vector3(Random.Range(-5f, 5f), Random.Range(-5f, 5f), Random.Range(-5f, 5f)), Quaternion.identity);
            var radius = Random.value;
            body.transform.localScale = radius * Vector3.one;
            var bodyComponent = body.AddComponent<Body>();
            bodyComponent.velocity = Random.insideUnitSphere.normalized * 1000f;
            bodyComponent.mass = 1e6f * radius * radius * radius;
            bodies.Add(bodyComponent);
        }
    }
}
