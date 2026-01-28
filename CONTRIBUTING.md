# Contributing Guide

Welcoming contributions from the community!

[![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-orange?logo=rust)](https://www.rust-lang.org/) [![Documentation](https://img.shields.io/badge/docs-feluda-blue)](https://feluda.readthedocs.io/)

## Quick Start

1. Fork and clone the repository:

```sh
git clone https://github.com/yourusername/feluda.git
cd feluda
```

2. Build and run:

```sh
cargo build
./target/debug/feluda --help
```

3. Run tests:

```sh
cargo test
```

4. Setup pre-commit hooks (recommended):

```sh
just setup
```

## Documentation

For detailed contributing guidelines, please visit our documentation:

- **[Development Setup](https://feluda.readthedocs.io/en/latest/contributing/setup/)** - Complete setup instructions
- **[Testing Guide](https://feluda.readthedocs.io/en/latest/contributing/testing/)** - Running tests and example projects
- **[Architecture](https://feluda.readthedocs.io/en/latest/contributing/architecture/)** - Codebase structure and design
- **[Adding Languages](https://feluda.readthedocs.io/en/latest/contributing/adding-languages/)** - How to add support for new languages
- **[License Matrix](https://feluda.readthedocs.io/en/latest/contributing/license-matrix/)** - Maintaining the compatibility matrix

## Submitting Changes

1. Create a branch: `git checkout -b feat/my-feature`
2. Make your changes and commit: `git commit -m "add: feature"`
3. Push and open a PR: `git push origin feat/my-feature`

## Reporting Issues

Found a bug or have a feature request? [Open an issue](https://github.com/anistark/feluda/issues) on GitHub.

---

_Minimum Supported Rust Version: `1.85.0`_
