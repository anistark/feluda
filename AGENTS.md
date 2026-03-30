# AGENTS.md — AI Coding Agent Instructions for Feluda

> Instructions for Claude Code, pi, Cursor, Copilot, and other AI coding agents working on this project.

---

## Project Overview

**Feluda** is a Rust-based CLI tool that analyzes project dependencies, identifies their licenses, and flags any that restrict personal or commercial usage or are incompatible with your project's license.

- **Repository:** https://github.com/anistark/feluda
- **Crate:** https://crates.io/crates/feluda
- **Docs:** https://feluda.readthedocs.io
- **License:** MIT
- **Minimum Rust Version:** 1.85
- **Recommended Rust Version:** Latest stable

---

## ⚠️ Core Concepts — Read This First

### What Feluda Does

Feluda scans a project's dependency files, resolves each dependency's license (from local files or the GitHub API), and produces a report. It supports **eight language ecosystems**, multiple output formats, SBOM generation, and CI/CD integration.

### The Analysis Pipeline

```
User invokes feluda [--path ./project] [--language rust] [flags...]
        ↓
src/parser.rs — discover project files, detect language(s)
        ↓
src/languages/<lang>.rs — parse dependency manifest, resolve licenses
        ↓ (local file check first, then GitHub API fallback)
src/licenses.rs — enrich with compatibility, OSI status, restrictiveness
        ↓
src/reporter.rs — format output (text/JSON/YAML/CI/gist)
   or src/table.rs — TUI mode (ratatui)
   or src/generate.rs — NOTICE/THIRD_PARTY_LICENSES files
   or src/sbom/ — SPDX/CycloneDX generation
```

### Language Ecosystems

| Language | Manifest File(s) | Parser Module | Local License Detection |
|----------|-------------------|---------------|------------------------|
| **Rust** | `Cargo.toml` | `src/languages/rust.rs` | `Cargo.toml` license field |
| **Node.js** | `package.json` | `src/languages/node.rs` | `node_modules/*/LICENSE` files |
| **Go** | `go.mod` | `src/languages/go.rs` | — |
| **Python** | `requirements.txt`, `Pipfile.lock`, `pip_freeze.txt`, `pyproject.toml` | `src/languages/python.rs` | — |
| **C** | `configure.ac`, `configure.in`, `Makefile` | `src/languages/c.rs` | — |
| **C++** | `vcpkg.json`, `conanfile.txt`, `CMakeLists.txt`, `MODULE.bazel` | `src/languages/cpp.rs` | — |
| **R** | `DESCRIPTION`, `renv.lock` | `src/languages/r.rs` | — |
| **.NET** | `.csproj`, `.fsproj`, `.vbproj`, `.slnx` | `src/languages/dotnet.rs` | — |

### Critical Rules

1. **Local-first license resolution.** Feluda checks local files before making network requests. The `--no-local` flag overrides this. Respect this order in all language parsers.

2. **GitHub API rate limits matter.** Unauthenticated: 60 req/hr. Authenticated (`--github-token`): 5,000 req/hr. Never make unnecessary API calls. Use the cache system (`src/cache.rs`).

3. **License compatibility is configurable.** The compatibility matrix lives in `config/license_compatibility.toml`. User overrides come from `.feluda.toml` and environment variables. Don't hardcode compatibility rules.

4. **Configuration precedence:** Environment variables > `.feluda.toml` > defaults. This is handled by `figment` in `src/config.rs`. Don't bypass this chain.

5. **Each language parser is self-contained.** A language module in `src/languages/` handles discovery, parsing, and license resolution for its ecosystem. Don't add cross-language coupling.

6. **Error types live in `src/debug.rs`.** All custom errors use `FeludaError` (thiserror-based). Add new variants there, not in individual modules.

---

## After Every Set of Changes

After completing any set of changes, **always** run these in order:

1. **`just format`** — Format all Rust code.
2. **`just lint`** — Run clippy (zero warnings enforced) and rustfmt check.
3. **`just test`** — Run the full test suite.

For a full CI-equivalent check:

4. **`just test-ci`** — Runs format-check → clippy → test (mirrors GitHub Actions).

Do not consider a change complete until all of the above pass cleanly.

### Additional Housekeeping

- **Prefer `just` commands** over raw `cargo` commands. The justfile handles sequencing correctly.
- **Prompt the user if `AGENTS.md` needs updating.** If your changes alter architecture, CLI commands, language support, key file locations, or behavioural conventions, tell the user: *"This change may require an update to AGENTS.md — would you like me to update it?"*

### Planning Documents

- **Check `plan/` for active plans** when a related task is mentioned. Files like `plan/ROADMAP.md` and `plan/*_IMPLEMENTATION.md` contain detailed implementation plans, checklists, and phase tracking.
- **`plan/` is for local planning only.** It is gitignored — never commit it. Use it to understand context, track progress, and follow implementation checklists.
- **Create new planning docs here** when scoping features or multi-step work (e.g., `plan/NEW_LANG_SWIFT.md`, `plan/SBOM_V2.md`). Keep them focused — one doc per initiative.

### Git Discipline

- **Do not run `git add` or `git commit` unless the user explicitly asks.** Stage and commit only on direct request.
- **When the user asks to commit**, review all staged/unstaged changes and prepare:
  - A **brief title** following conventional commits (`feat:`, `fix:`, `chore:`, etc.)
  - A **detailed description** summarizing what changed and why

---

## Architecture

Feluda is a single Rust binary with a Sphinx documentation site:

| Component | Language | Location | Purpose |
|-----------|----------|----------|---------|
| **Core CLI** | Rust | `src/` | CLI, parsing, analysis, reporting |
| **License Matrix** | TOML | `config/` | License compatibility rules |
| **Documentation** | Sphinx (RST) | `docs/` | User-facing documentation |
| **Examples** | Multi-language | `examples/` | Test projects for each supported language |
| **GitHub Action** | YAML | `action.yml` | CI/CD integration for GitHub |

### Source Layout (`src/`)

```
src/
├── main.rs              # Entry point, command dispatch, CheckConfig
├── cli.rs               # CLI argument parsing (clap derive), LoadingIndicator
├── debug.rs             # FeludaError enum, FeludaResult, debug logging
├── config.rs            # .feluda.toml + env var config (figment)
├── parser.rs            # Project discovery, language detection, parse coordination
├── licenses.rs          # License analysis, compatibility, OSI status, GitHub API
├── cache.rs             # GitHub license data caching (.feluda/cache/)
├── reporter.rs          # Text/JSON/YAML/CI/gist output formatting
├── table.rs             # TUI mode (ratatui)
├── generate.rs          # NOTICE / THIRD_PARTY_LICENSES file generation
├── utils.rs             # Git clone, path utilities
├── progress.rs          # Progress display utilities
├── languages/
│   ├── mod.rs           # Language enum, LanguageParser trait, file patterns
│   ├── rust.rs          # Rust/Cargo dependency analysis
│   ├── node.rs          # Node.js/npm/pnpm/yarn/bun dependency analysis
│   ├── go.rs            # Go module dependency analysis
│   ├── python.rs        # Python dependency analysis
│   ├── c.rs             # C dependency analysis
│   ├── cpp.rs           # C++ dependency analysis
│   ├── r.rs             # R dependency analysis
│   └── dotnet.rs        # .NET dependency analysis
└── sbom/
    ├── mod.rs           # SBOM command handler, shared types
    ├── spdx.rs          # SPDX 2.3 format generation
    ├── cyclonedx.rs     # CycloneDX v1.5 format generation
    └── validate/
        ├── mod.rs       # Validation command handler
        ├── parser.rs    # SBOM file parsing
        ├── spdx_validator.rs    # SPDX validation rules
        ├── cyclonedx_validator.rs  # CycloneDX validation rules
        └── reporter.rs  # Validation report formatting
```

### Key Architectural Patterns

- **Language detection via file patterns.** `src/languages/mod.rs` defines `Language::from_file_name()` which maps manifest filenames to language variants. `src/parser.rs` scans the project root for these files.
- **Parallel analysis.** Multiple project roots are analyzed in parallel using `rayon`.
- **Two-tier license resolution.** Local files are checked first (e.g., `node_modules/*/LICENSE`, `Cargo.toml` license field), then GitHub API as fallback. The `--no-local` flag skips local checks.
- **Caching.** GitHub API responses are cached in `.feluda/cache/github_licenses.json` with 30-day expiration.
- **Configuration layering.** `figment` merges defaults → `.feluda.toml` → environment variables. See `src/config.rs`.
- **Error handling.** `thiserror`-based `FeludaError` in `src/debug.rs` with `FeludaResult<T>` alias. Debug mode (`--debug`) enables verbose logging.

---

## Documentation Structure

```
docs/
├── source/              # Sphinx RST source files
│   ├── conf.py          # Sphinx configuration
│   ├── index.rst        # Documentation root
│   └── ...              # RST pages
├── requirements.txt     # Python dependencies (Sphinx, etc.)
└── build/               # Generated output (gitignored)
```

Documentation is hosted on ReadTheDocs. When updating docs, place content in `docs/source/` and follow RST formatting.

---

## Tech Stack & Tooling

| Tool | Purpose | Notes |
|------|---------|-------|
| **Rust** | Core CLI | Edition 2021, MSRV 1.85 |
| **Cargo** | Build system | `cargo build --release` |
| **Just** | Task runner | `justfile` — run `just` for available commands |
| **clap** | CLI parsing | Derive-based, see `src/cli.rs` |
| **thiserror** | Error types | `FeludaError` in `src/debug.rs` |
| **figment** | Configuration | Layered config: TOML + env vars |
| **reqwest** | HTTP client | For GitHub API calls (blocking mode) |
| **rayon** | Parallelism | Parallel dependency analysis |
| **ratatui** | TUI framework | Interactive terminal UI (`--gui`) |
| **serde** / **serde_json** / **serde_yaml** | Serialization | JSON/YAML output, config parsing |
| **cargo_metadata** | Rust analysis | Cargo dependency resolution |
| **Sphinx** | Documentation | RST-based, deployed to ReadTheDocs |
| **clippy** | Linting | Enforced: `-D warnings` (zero warnings policy) |
| **cargo fmt** | Formatting | Standard rustfmt |

---

## Build & Development

### Quick Commands

```sh
just build          # Format → lint → test → release build
just test           # Run all Rust tests
just format         # Format Rust code
just lint           # Clean → fmt check → clippy
just test-ci        # Full CI check (format → clippy → test)
just clean          # Remove build artifacts
just setup          # Configure git hooks
just install        # Build + install to /usr/local/bin
```

### Building from Source

```sh
# Full build
cargo build --release

# Run tests
cargo test

# Run specific test
cargo test test_name

# Run with debug output
cargo run -- --debug --path ./examples/rust-example
```

### Documentation

```sh
just docs-setup     # Install Sphinx + dependencies (one-time)
just docs-build     # Build HTML documentation
just docs-serve     # Local dev server with live reload
just docs-check     # Lint RST + strict build + link check
```

### Testing with Example Projects

```sh
just test-examples  # Run feluda against all example projects

# Or manually:
cargo run -- --path examples/rust-example
cargo run -- --path examples/node-example
cargo run -- --path examples/go-example
cargo run -- --path examples/python-example
```

---

## Testing

- **Unit tests** live alongside source code (standard Rust `#[cfg(test)]` modules).
- **Dev dependencies** include `tempfile`, `mockall`, `http`, `temp-env`, `serial_test`.
- Always run `cargo test` before committing.
- The CI expects zero clippy warnings: `cargo clippy --all-targets --all-features -- -D warnings`.

---

## Code Conventions

### Rust

- Follow standard Rust naming: `snake_case` for functions/variables, `PascalCase` for types.
- Use `thiserror` for error types. Add new variants to `FeludaError` in `src/debug.rs`.
- Keep `#[allow(dead_code)]` annotated with a `// TODO:` comment or trait explanation.
- Use `log()` / `log_debug()` / `log_error()` from `src/debug.rs` for debug output (only prints when `--debug` is active).
- The `LoadingIndicator` / `with_spinner()` in `src/cli.rs` provides user-facing progress. Use it for long operations.
- Language parsers should implement the analysis pattern: detect → parse manifest → resolve licenses (local then network).

### Commit Messages

Follow conventional commits:

```
feat: description          # New feature
fix: description           # Bug fix
chore: description         # Maintenance, deps, CI
docs: description          # Documentation only
refactor: description      # Code restructuring
test: description          # Adding/fixing tests
```

### Branching

- `main` — stable release branch
- `feat/*` — feature branches
- `fix/*` — bug fix branches
- `chore/*` — maintenance branches
- `docs/*` — documentation branches

---

## Versioning

- Version source of truth: `Cargo.toml` (`version = "X.Y.Z"`).
- Follow [Semantic Versioning](https://semver.org/).
- `just publish` handles: build → test → package → crates.io publish → git tag → push tag.

---

## CLI Commands Reference

```sh
# Default: License analysis
feluda                                    # Analyze current directory
feluda --path /path/to/project            # Analyze specific path
feluda --language rust                    # Force language detection
feluda --repo https://github.com/user/repo  # Analyze remote repo

# Output formats
feluda --json                             # JSON output
feluda --yaml                             # YAML output
feluda --gist                             # Concise summary
feluda --verbose                          # Detailed with OSI status
feluda --gui                              # Interactive TUI mode

# Filtering
feluda --restrictive                      # Show only restrictive licenses
feluda --incompatible                     # Show only incompatible licenses
feluda --osi approved                     # Filter by OSI status
feluda --project-license MIT              # Check compatibility against MIT

# CI/CD
feluda --fail-on-restrictive              # Exit 1 if restrictive found
feluda --fail-on-incompatible             # Exit 1 if incompatible found
feluda --ci-format github                 # GitHub Actions output
feluda --ci-format jenkins                # JUnit XML output
feluda --output-file report.txt           # Write to file

# Subcommands
feluda generate                           # Generate NOTICE / THIRD_PARTY_LICENSES
feluda sbom                               # Generate all SBOM formats
feluda sbom spdx --output sbom.json       # Generate SPDX SBOM
feluda sbom cyclonedx --output sbom.json  # Generate CycloneDX SBOM
feluda sbom validate sbom.json            # Validate SBOM file
feluda cache                              # Show cache status
feluda cache --clear                      # Clear cache

# Options
feluda --github-token <token>             # Authenticated API requests
feluda --no-local                         # Skip local license detection
feluda --strict                           # Strict license parsing
feluda --debug                            # Enable debug logging
```

---

## Important Files to Know

| File | Why It Matters |
|------|----------------|
| `Cargo.toml` | Version, dependencies, metadata — start here |
| `src/cli.rs` | All CLI arguments and subcommands (clap derive) |
| `src/main.rs` | Command dispatch, `CheckConfig`, `run()` |
| `src/debug.rs` | `FeludaError` enum, `FeludaResult`, debug logging |
| `src/config.rs` | Configuration loading (figment: defaults → TOML → env) |
| `src/parser.rs` | Project discovery, language detection, parse coordination |
| `src/licenses.rs` | License analysis, compatibility checking, GitHub API |
| `src/languages/mod.rs` | `Language` enum, `LanguageParser` trait, file patterns |
| `src/languages/*.rs` | Per-language dependency parsers |
| `src/reporter.rs` | Output formatting (text, JSON, YAML, CI, gist) |
| `src/table.rs` | TUI interface (ratatui) |
| `src/sbom/mod.rs` | SBOM generation entry point |
| `src/cache.rs` | GitHub license data caching |
| `config/license_compatibility.toml` | License compatibility matrix |
| `action.yml` | GitHub Action definition |
| `justfile` | All development task commands |
| `.feluda.toml` | User configuration (restrictive overrides, ignores) |

---

## Common Tasks for Agents

### Adding a new language

1. Create `src/languages/<lang>.rs` with an `analyze_<lang>_licenses()` function.
2. Add the `Language` variant to the `Language` enum in `src/languages/mod.rs`.
3. Add file pattern matching in `Language::from_file_name()`.
4. Add file pattern constants if needed (e.g., `LANG_PATHS`).
5. Wire up the parser in `src/parser.rs` (import + match arm in the scanning logic).
6. Export the module in `src/languages/mod.rs`.
7. Add a test example project in `examples/<lang>-example/`.
8. Update `just test-examples` in `justfile`.
9. Add tests.
10. Update docs and README.

### Adding a new CLI flag

1. Add the field to the `Cli` struct in `src/cli.rs`.
2. Wire it through `CheckConfig` in `src/main.rs` if it affects analysis.
3. Handle it in the appropriate command handler.
4. Add tests.
5. Update README usage section.

### Adding a new subcommand

1. Add the variant to `Commands` enum in `src/cli.rs`.
2. Create handler function (in existing module or new module).
3. Add match arm in `run()` in `src/main.rs`.
4. Add tests.
5. Update README and docs.

### Adding a new error type

1. Add a variant to `FeludaError` in `src/debug.rs`.
2. Use `thiserror` `#[error("...")]` for the display message.
3. Add `#[from]` if it wraps a standard error type.

### Adding a new output format

1. Add the format logic in `src/reporter.rs` (or a new module).
2. Add CLI flag in `src/cli.rs`.
3. Wire through `ReportConfig` in `src/reporter.rs`.
4. Add tests.

### Modifying the license compatibility matrix

1. Edit `config/license_compatibility.toml`.
2. Test with `cargo run -- --project-license <LICENSE> --path examples/rust-example`.
3. **Consult legal expertise** — compatibility rules have legal implications.

---

## Gotchas & Pitfalls

- **`*.json` is gitignored.** The `.gitignore` includes `*.json`, so JSON files in the repo root won't be tracked. Config JSON files should use TOML instead.
- **clippy must pass with zero warnings** — CI enforces `-D warnings`.
- **The `--debug` flag controls logging.** All `log()`, `log_debug()`, `log_error()` calls in `src/debug.rs` are no-ops unless `--debug` is active.
- **`reqwest` is in blocking mode.** Despite `tokio` being a dependency, HTTP calls use `reqwest::blocking`. This is intentional for CLI simplicity.
- **`serial_test` is used for tests that share global state.** Some tests modify static state (e.g., debug mode, GitHub token). Use `#[serial]` for these.
- **The `progress` module and `LoadingIndicator`** write directly to stdout with ANSI escape codes. They're skipped in debug mode to avoid garbled output.
- **Configuration validation is strict.** Duplicate licenses in restrictive/ignore lists, empty license strings, and licenses appearing in both lists are caught. See `src/config.rs`.
- **GitHub Action (`action.yml`)** uses the published crate, not a source build. Keep it in sync with CLI changes.
- **`plan/` is gitignored.** Local planning only — never commit anything in it.

---

## CI Workflows

| Workflow | Trigger | What it does |
|----------|---------|-------------|
| `ci.yml` | Push to main, PRs | Format check + clippy + tests |
| `release-binaries.yml` | Release created | Builds binaries for Linux/macOS/Windows, creates DEB/RPM packages |

---

## Examples

The `examples/` directory contains test projects for each supported language:

- `rust-example/` — Rust project with `Cargo.toml`
- `node-example/` — Node.js project with `package.json`
- `go-example/` — Go project with `go.mod`
- `python-example/` — Python project with `requirements.txt`
- `c-example/` — C project
- `cpp-example/` — C++ project
- `r-example/` — R project
- `dotnet-example/` — .NET project
- `ci/` — CI integration examples (GitHub Actions, Jenkins)

Use these for testing. Run `just test-examples` to validate against all of them.
