cmake_minimum_required(VERSION 3.16)
project(n_body)

set(CMAKE_CXX_STANDARD 20)

if(NOT MSVC)
    find_package(TBB REQUIRED)
endif()

add_executable(n_body main.cpp)

if(NOT MSVC)
    target_link_libraries(n_body PRIVATE TBB::tbb)
endif()