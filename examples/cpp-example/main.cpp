#include <iostream>
#include <fmt/core.h>
#include <nlohmann/json.hpp>
#include <spdlog/spdlog.h>
#include <boost/system/error_code.hpp>

using json = nlohmann::json;

int main() {
    // Example usage of libraries with transient dependencies
    fmt::print("C++ example with transient dependencies\n");

    spdlog::info("Using spdlog for logging");

    json j = {
        {"message", "C++ example with transient dependencies"},
        {"libraries", {"fmt", "nlohmann-json", "spdlog", "boost"}}
    };

    std::cout << j.dump(2) << std::endl;

    return 0;
}
