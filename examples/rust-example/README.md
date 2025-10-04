# Rust Example Project

This is a sample Rust project used for testing Feluda's license analysis capabilities.

## Dependencies

This project includes dependencies with transient (indirect) dependencies:
- **serde**: Serialization framework (has transient dependencies like serde_derive)
- **tokio**: Async runtime (has transient dependencies like tokio-macros, mio, etc.)
- **reqwest**: HTTP client (has transient dependencies like hyper, http, etc.)
- **clap**: CLI argument parser (has transient dependencies like clap_derive, etc.)

## Testing with Feluda

Run Feluda on this project:

```sh
feluda --path examples/rust-example
```

Or from within the example directory:

```sh
cd examples/rust-example
feluda
```
