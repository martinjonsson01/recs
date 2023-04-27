#include <chrono>
#include <eigen3/Eigen/Eigen>
#include <entt/entt.hpp>
#include <execution>
#include <iostream>
#include <random>

const float FIXED_TIME_STEP = 1.0 / 20000.0;

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

struct Benchmark {
  int body_count;
  int ticks_per_measurement;
  int measurement_count;

  Benchmark(int body_count, int simulated_ticks, int measurement_count = 10) : body_count(body_count), measurement_count(measurement_count) {
      ticks_per_measurement = simulated_ticks / measurement_count;
  }

  void run() const {
      entt::registry registry;

      std::default_random_engine rnd;
      std::uniform_real_distribution<float> radius_distribution(0, 1);
      std::uniform_real_distribution<float> position_distribution(-5, 5);

      for (int i = 0; i < body_count; i++) {
          const auto entity = registry.create();
          auto radius = radius_distribution(rnd);
          registry.emplace<Position>(entity, Eigen::Vector3f(position_distribution(rnd), position_distribution(rnd), position_distribution(rnd)));
          registry.emplace<Velocity>(entity, Eigen::Vector3f::Zero());
          registry.emplace<Acceleration>(entity, Eigen::Vector3f::Zero());
          registry.emplace<Mass>(entity, 1e6f * radius * radius * radius);
      }
      
      std::vector<std::chrono::duration<double>> measurements;

      for (int i = 0; i < measurement_count; i++) {
          auto start = std::chrono::high_resolution_clock::now();
          for (int j = 0; j < ticks_per_measurement; j++) {
              update(registry);
          }
          auto stop = std::chrono::high_resolution_clock::now();
          measurements.emplace_back(stop - start);
          std::cout << "." << std::flush;
      }
      
      double mean = 0;
      for (auto& measurement : measurements) {
          mean += measurement.count();
      }
      mean /= measurement_count;
      
      double variance = 0;
      for (auto& measurement : measurements) {
          auto val = measurement.count();
          variance += (val - mean) * (val - mean);
      }
      variance /= measurement_count - 1;
      auto standard_deviation = sqrt(variance);
      
      std::cout << "\rBenchmark result for " << body_count << " bodies over " << ticks_per_measurement * measurement_count << " ticks:" << std::endl;
      std::cout << "Avg. TPS: " << ticks_per_measurement / mean << " Avg. tick time (ms): " << mean << " Standard deviation (ms): " << standard_deviation << std::endl << std::endl;
  }
};

int main() {
    Benchmark benchmarks[] = {
        Benchmark(100, 100000),
        Benchmark(500, 4000),
        Benchmark(1000, 1000),
        Benchmark(5000, 400),
        Benchmark(10000, 100),
        Benchmark(100000, 10),
    };

    for (auto& benchmark : benchmarks) {
        benchmark.run();
    }

    return 0;
}
