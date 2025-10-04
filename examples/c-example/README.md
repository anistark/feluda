# C Example Project

This is a sample C project used for testing Feluda's license analysis capabilities.

## Dependencies

This project uses system libraries that have transient (indirect) dependencies:
- **openssl**: Cryptography library (has transient dependencies on libcrypto)
- **libcurl**: HTTP client library (has transient dependencies on openssl, zlib, etc.)
- **zlib**: Compression library (standalone but commonly used)

## Testing with Feluda

Run Feluda on this project:

```sh
feluda --path examples/c-example
```

Or from within the example directory:

```sh
cd examples/c-example
feluda
```

## Setup (Optional)

To actually build and run this example:

### Ubuntu/Debian
```sh
sudo apt-get install libssl-dev libcurl4-openssl-dev zlib1g-dev
make
./c_example
```

### macOS
```sh
brew install openssl curl zlib
make
./c_example
```

## Note

C dependency detection relies on system package managers (apt, dnf, pacman) and pkg-config.
The Makefile demonstrates typical C project structure with library dependencies.
