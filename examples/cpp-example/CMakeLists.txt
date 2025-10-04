cmake_minimum_required(VERSION 3.15)
project(cpp_example)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Find packages
find_package(fmt CONFIG REQUIRED)
find_package(nlohmann_json CONFIG REQUIRED)
find_package(spdlog CONFIG REQUIRED)
find_package(Boost REQUIRED COMPONENTS system)

# Add executable
add_executable(cpp_example main.cpp)

# Link libraries
target_link_libraries(cpp_example PRIVATE
    fmt::fmt
    nlohmann_json::nlohmann_json
    spdlog::spdlog
    Boost::system
)
