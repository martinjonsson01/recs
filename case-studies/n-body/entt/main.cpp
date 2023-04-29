#include <chrono>
#include <eigen3/Eigen/Eigen>
#include <entt/entt.hpp>
#include <execution>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <random>

const float FIXED_TIME_STEP = 1.0 / 20000.0;
const int SAMPLE_COUNT = 10;
const int LARGEST_EXPONENT = 14;

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

        auto acceleration_towards_body = [&position](const Position& body_position,
                                                     const Mass& body_mass) -> Eigen::Vector3f {
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
    int ticks_per_sample;
    std::chrono::duration<double> samples[SAMPLE_COUNT]{};

    Benchmark(int body_count, int simulated_ticks) : body_count(body_count) {
        ticks_per_sample = simulated_ticks / SAMPLE_COUNT;
    }

    void run() {
        entt::registry registry;

        std::default_random_engine rnd;
        std::uniform_real_distribution<float> radius_distribution(0, 1);
        std::uniform_real_distribution<float> position_distribution(-5, 5);

        for (int i = 0; i < body_count; i++) {
            const auto entity = registry.create();
            auto radius = radius_distribution(rnd);
            registry.emplace<Position>(entity, Eigen::Vector3f(position_distribution(rnd), position_distribution(rnd),
                                                               position_distribution(rnd)));
            registry.emplace<Velocity>(entity, Eigen::Vector3f::Zero());
            registry.emplace<Acceleration>(entity, Eigen::Vector3f::Zero());
            registry.emplace<Mass>(entity, 1e6f * radius * radius * radius);
        }

        for (int i = -1; i < SAMPLE_COUNT; i++) {
            auto start = std::chrono::high_resolution_clock::now();
            for (int j = 0; j < ticks_per_sample; j++) {
                update(registry);
            }
            auto stop = std::chrono::high_resolution_clock::now();
            if (i >= 0) samples[i] = stop - start;
            std::cout << "." << std::flush;
        }

        std::cout << "\rFinished benchmark with " << body_count << (body_count > 1 ? " bodies" : " body") << "."
                  << std::endl;
    }
};

struct numpunct : std::numpunct<char> {
    char do_decimal_point() const override { return ','; }
};

void save_as_csv(const std::vector<Benchmark>& benchmarks) {
    std::filesystem::path path = "entt.csv";
    std::cout << "Writing benchmark results to " << std::filesystem::absolute(path) << "." << std::endl;

    std::ofstream csv;
    csv.open(path);
    csv.imbue(std::locale(std::cout.getloc(), new numpunct));

    csv << "body_count,ticks_per_sample";
    for (int i = 0; i < SAMPLE_COUNT; i++) csv << ",sample_" << i;
    csv << std::endl;

    for (auto& benchmark : benchmarks) {
        csv << benchmark.body_count << "," << benchmark.ticks_per_sample;
        for (auto& sample : benchmark.samples) {
            csv << ",\"" << std::fixed << std::setprecision(15) << sample.count() / benchmark.ticks_per_sample << '"';
        }
        csv << std::endl;
    }

    csv.close();
}

int main() {
    std::vector<Benchmark> benchmarks;

    for (int n = 0; n <= LARGEST_EXPONENT; n++) {
        benchmarks.emplace_back(1 << n, 1000);
    }

    for (auto& benchmark : benchmarks) {
        benchmark.run();
    }

    save_as_csv(benchmarks);

    return 0;
}
