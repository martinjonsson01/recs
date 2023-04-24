# Build instructions for CLion

- Install the compiler
  - Windows
    - Install MSVC using Visual Studio Installer
  - Linux
    - Install GCC or Clang from the package manager
- Install dependencies
  - Windows
    - Create a directory called `include` that can be placed anywhere.
    - Download EnTT header from https://raw.githubusercontent.com/skypjack/entt/master/single_include/entt/entt.hpp and place it in `include/entt/entt.hpp`.
    - Download Eigen3 from https://eigen.tuxfamily.org/index.php, place the folder called `Eigen` here `include/eigen3/Eigen`.
  - Linux
    - Install EnTT and Eigen3 from the package manager or manually place them in `/usr/local/include`.
- Configure CMake
  - In CLion, navigate to `Edit > Settings > Build, Execution, Deployment > CMake`
  - Press `+` to add Release configuration
  - On Windows, add this CMake option on both Debug and Release, `-DCMAKE_CXX_FLAGS=-I\ path\to\include` where `path\to\include` is the include-directory created in a previous step.
- Set default toolchain
  - In CLion, navigate to `Edit > Settings > Build, Execution, Deployment > Toolchains`
  - The toolchain on the top of the one being used.
  - On Windows, make sure that that Visual Studio is the default.
- Load CMake project
  - Open `case-studies/n-body/entt/CMakeLists.txt`, click `Load CMake project` in the top right corner.
- You can now build it from CLion.

# Running the benchmark
The constants `BODY_COUNT` and `SIMULATED_TICKS` can be changed at the top of the `main.cpp` file.
Then just compile and run the benchmark.