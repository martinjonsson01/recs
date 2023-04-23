using System.Collections.Generic;
using UnityEngine;
using static Constants;

public class Body : MonoBehaviour
{
    public float mass;
    public Vector3 velocity;

    Vector3 CalculateAccelerationTowardsBody(Body other)
    {
        var toBody = other.transform.position - transform.position;
        var distanceSquared = toBody.sqrMagnitude;

        if (distanceSquared <= float.Epsilon) return Vector3.zero;

        var acceleration = GRAVITATIONAL_CONSTANT * other.mass / distanceSquared;
        return toBody.normalized * acceleration;
    }

    public void UpdatePosition(List<Body> bodies)
    {
        var netAcceleration = Vector3.zero;

        foreach (var body in bodies)
        {
            netAcceleration += CalculateAccelerationTowardsBody(body);
        }

        velocity += netAcceleration * FIXED_TIME_STEP;
        transform.position += velocity * FIXED_TIME_STEP;
    }
}
