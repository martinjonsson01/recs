#include <chrono>
#include <eigen3/Eigen/Eigen>
#include <entt/entt.hpp>
#include <execution>
#include <iostream>
#include <random>

const int BODY_COUNT = 10000;
const float FIXED_TIME_STEP = 1.0 / 20000.0;
const int SIMULATED_TICKS = 100;

struct Position {
    Eigen::Vector3f point;
};

struct Velocity {
    Eigen::Vector3f vector;
};

struct Acceleration {
    Eigen::Vector3f vector;
};

struct Mass {
    float scalar;
};

void movement(entt::registry& registry) {
    auto view = registry.view<Position, const Velocity>();
    auto& handle = view.handle();

    std::for_each(std::execution::par, handle.begin(), handle.end(), [&](auto entity) {
        auto& position = view.get<Position>(entity);
        const auto& velocity = view.get<Velocity>(entity);

        position.point += velocity.vector * FIXED_TIME_STEP;
    });
}

void acceleration(entt::registry& registry) {
    auto view = registry.view<Velocity, const Acceleration>();
    auto& handle = view.handle();

    std::for_each(std::execution::par, handle.begin(), handle.end(), [&](auto entity) {
        auto& velocity = view.get<Velocity>(entity);
        const auto& acceleration = view.get<Acceleration>(entity);

        velocity.vector += acceleration.vector * FIXED_TIME_STEP;
    });
}

void gravity(entt::registry& registry) {
    auto view = registry.view<const Position, Acceleration>();
    auto& handle = view.handle();
    auto bodies_view = registry.view<const Position, const Mass>();

    std::for_each(std::execution::par, handle.begin(), handle.end(), [&](auto entity) {
        const auto& position = view.get<const Position>(entity);
        auto& acceleration = view.get<Acceleration>(entity);

        auto acceleration_towards_body = [&position](const Position& body_position, const Mass& body_mass) -> Eigen::Vector3f {
            auto to_body = body_position.point - position.point;
            auto distance_squared = to_body.squaredNorm();

            if (distance_squared <= std::numeric_limits<float>::epsilon()) return Eigen::Vector3f::Zero();

            const float GRAVITATIONAL_CONSTANT = 6.67e-11;
            auto acceleration = GRAVITATIONAL_CONSTANT * body_mass.scalar / distance_squared;

            return to_body.normalized() * acceleration;
        };

        Eigen::Vector3f netAcceleration = Eigen::Vector3f::Zero();

        bodies_view.each([&](const Position& body_position, const Mass& body_mass) {
            netAcceleration += acceleration_towards_body(body_position, body_mass);
        });

        acceleration.vector = netAcceleration;
    });
}

void update(entt::registry& registry) {
    gravity(registry);
    acceleration(registry);
    movement(registry);
}

int main() {
    entt::registry registry;

    std::default_random_engine rnd;
    std::uniform_real_distribution<float> radius_distribution(0, 1);
    std::uniform_real_distribution<float> position_distribution(-5, 5);

    for (int i = 0; i < BODY_COUNT; i++) {
        const auto entity = registry.create();
        auto radius = radius_distribution(rnd);
        registry.emplace<Position>(entity, Eigen::Vector3f(position_distribution(rnd), position_distribution(rnd), position_distribution(rnd)));
        registry.emplace<Velocity>(entity, Eigen::Vector3f::Zero());
        registry.emplace<Acceleration>(entity, Eigen::Vector3f::Zero());
        registry.emplace<Mass>(entity, 1e6f * radius * radius * radius);
    }

    auto start = std::chrono::high_resolution_clock::now();

    for (int i = 0; i < SIMULATED_TICKS; i++) {
        update(registry);
    }

    auto stop = std::chrono::high_resolution_clock::now();

    std::chrono::duration<double> duration = stop - start;
    std::cout << "Benchmark took " << duration_cast<std::chrono::milliseconds>(duration).count() << " ms to complete" << std::endl;
    std::cout << SIMULATED_TICKS / duration.count() << " avg tps over " << SIMULATED_TICKS << " ticks" << std::endl;

    return 0;
}
