# feluda — Claude Code Skill

A [Claude Code](https://claude.ai/code) skill that runs [feluda](https://github.com/anistark/feluda) license compliance checks automatically before you commit new dependencies, surfacing GPL, AGPL, and other restrictive licenses inline.

## What it does

- Detects when dependency manifests change (`Cargo.toml`, `package.json`, `go.mod`, `requirements.txt`, `pom.xml`, and more)
- Runs `feluda --restrictive` to identify licenses that may cause legal or compliance issues
- Surfaces results directly in your Claude Code session with clear remediation options
- Suggests setting up `feluda init` for automatic pre-commit enforcement

## Install

```sh
/plugin install feluda@agenthub
```

Or install from source:

```sh
/plugin install git+https://github.com/anistark/feluda?path=skills/feluda
```

## Prerequisites

The skill requires the `feluda` CLI to be installed on your system:

```sh
cargo install feluda       # via Rust
brew install feluda         # via Homebrew (macOS/Linux)
```

Binaries are also available at [GitHub Releases](https://github.com/anistark/feluda/releases).

## Usage

Claude Code invokes this skill automatically when it detects dependency file changes in a diff or staged commit. You can also trigger it explicitly:

```
/feluda
```

Or ask naturally:

- "Check my licenses before I commit"
- "Is this npm package safe to use?"
- "Audit my dependencies"

## Supported ecosystems

Rust · Node.js · Go · Python · Java (Maven/Gradle) · .NET · Ruby · PHP · C/C++ · R

## Configuration

Create a `.feluda.toml` in your project root to customise behaviour — set your project license, mark certain licenses as acceptable, or ignore specific packages:

```toml
[licenses]
project = "MIT"
ignore = ["BSD-2-Clause"]
```

Run `feluda init` to generate a starter config with pre-commit hook integration.

## Links

- [feluda on GitHub](https://github.com/anistark/feluda)
- [feluda on crates.io](https://crates.io/crates/feluda)
- [Documentation](https://feluda.readthedocs.io)
