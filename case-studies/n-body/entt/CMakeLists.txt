cmake_minimum_required(VERSION 3.26)
project(n_body)

set(CMAKE_CXX_STANDARD 20)

find_package(TBB REQUIRED)

add_executable(n_body main.cpp)

target_link_libraries(n_body PRIVATE TBB::tbb)
