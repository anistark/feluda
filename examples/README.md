# Feluda Example Projects

This directory contains example projects for all supported languages in Feluda. Each example project is designed to test Feluda's license analysis capabilities with real-world dependencies that have transient (indirect) dependencies.

## Available Examples

1. Rust Example (`rust-example/`)
2. Node.js Example (`node-example/`)
3. Go Example (`go-example/`)
4. Python Example (`python-example/`)
5. C Example (`c-example/`)
6. C++ Example (`cpp-example/`)
7. R Example (`r-example/`)

## Using Just Commands

The project includes `just` commands for easy testing:

```sh
# Show available example commands
just examples

# Test Feluda on all example projects
just test-examples
```

## Testing Different Output Formats

```sh
# JSON output
feluda --path examples/rust-example --json

# YAML output
feluda --path examples/node-example --yaml

# Verbose mode with OSI status
feluda --path examples/go-example --verbose

# TUI/GUI mode
feluda --path examples/python-example --gui

# Gist mode
feluda --path examples/c-example --gist

# License compatibility check
feluda --path examples/cpp-example --project-license MIT

# R project analysis
feluda --path examples/r-example --verbose
```

## Contributing

When adding a new language support to Feluda:

1. Create a new example project in `examples/<language>-example/`
2. Include dependencies with transient dependencies
3. Add a README.md explaining the dependencies
4. Update this README.md to list the new example
5. Update `justfile` to include the new example in `test-examples`
6. Test the example: `feluda --path examples/<language>-example`
