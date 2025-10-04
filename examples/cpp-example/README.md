# C++ Example Project

This is a sample C++ project used for testing Feluda's license analysis capabilities.

## Dependencies

This project uses vcpkg for package management and includes dependencies with transient (indirect) dependencies:
- **boost-system**: Boost system library (has transient dependencies on boost-config, etc.)
- **fmt**: Modern C++ formatting library (standalone but good for testing)
- **nlohmann-json**: JSON library for C++ (standalone but good for testing)
- **spdlog**: Fast C++ logging library (may have transient dependencies on fmt)

## Testing with Feluda

Run Feluda on this project:

```sh
feluda --path examples/cpp-example
```

Or from within the example directory:

```sh
cd examples/cpp-example
feluda
```

## Setup (Optional)

To actually build and run this example, you'll need vcpkg:

### Install vcpkg
```sh
git clone https://github.com/Microsoft/vcpkg.git
./vcpkg/bootstrap-vcpkg.sh
```

### Build the project
```sh
cmake -B build -S . -DCMAKE_TOOLCHAIN_FILE=/path/to/vcpkg/scripts/buildsystems/vcpkg.cmake
cmake --build build
./build/cpp_example
```

## Alternative: Conan Support

You can also create a `conanfile.txt` for Conan package manager support:

```ini
[requires]
boost/1.83.0
fmt/10.1.1
nlohmann_json/3.11.2
spdlog/1.12.0

[generators]
CMakeDeps
CMakeToolchain
```
