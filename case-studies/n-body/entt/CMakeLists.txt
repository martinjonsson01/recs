cmake_minimum_required(VERSION 3.16)
project(n_body_entt)

set(CMAKE_CXX_STANDARD 20)
add_compile_options("-march=native")

if(NOT MSVC)
    find_package(TBB REQUIRED)
endif()

add_executable(n_body_entt main.cpp)

if(NOT MSVC)
    target_link_libraries(n_body_entt PRIVATE TBB::tbb)
endif()
