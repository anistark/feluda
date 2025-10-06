# Contributing Guide

Welcoming contributions from the community! 🙌

[![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-orange?logo=rust)](https://www.rust-lang.org/)

_Minimum Supported Rust Version: `1.70.0`_
_Currently Working Rust Version: `1.88.0`_

### Folder Structure:

```sh
feluda/
├── src/
│   ├── main.rs              # Entry point
│   ├── cli.rs               # CLI argument handling
│   ├── config.rs            # Configuration management
│   ├── debug.rs             # Debug and logging utilities
│   ├── parser.rs            # Dependency parsing coordination
│   ├── licenses.rs          # License analysis, compatibility, and OSI integration
│   ├── reporter.rs          # Output formatting and reporting
│   ├── table.rs             # TUI components
│   ├── generate.rs          # License file generation
│   ├── utils.rs             # Git repository utilities
│   └── languages/           # Language-specific parsers
│       ├── mod.rs           # Language detection and common traits
│       ├── c.rs             # C project support
│       ├── cpp.rs           # C++ project support
│       ├── rust.rs          # Rust/Cargo support
│       ├── node.rs          # Node.js/npm support
│       ├── go.rs            # Go modules support
│       └── python.rs        # Python package support
├── examples/                # Example projects for testing
├── config/
│   └── license_compatibility.toml  # License compatibility matrix
├── Cargo.toml               # Project metadata
├── LICENSE                  # Project license
├── README.md                # Project documentation
├── justfile                 # Build automation
└── CONTRIBUTING.md
```

### Setting Up for Development

1. Fork the repository and clone it to your local machine:

```sh
git clone https://github.com/yourusername/feluda.git
cd feluda
```

2. Install dependencies and tools:

```sh
cargo build
```

3. Run locally

```sh
./target/debug/feluda --help
```

4. Run tests to ensure everything is working:

```sh
cargo test
```

### Testing with Example Projects

Feluda includes example projects for all supported languages in the `examples/` directory. These projects are designed to test Feluda's license analysis capabilities with real-world dependencies that have transient (indirect) dependencies.

#### Available Example Projects

1. **Rust Example** (`examples/rust-example/`): Uses serde, tokio, reqwest, and clap
2. **Node.js Example** (`examples/node-example/`): Uses express, axios, lodash, and moment
3. **Go Example** (`examples/go-example/`): Uses gin, cobra, testify, and zap
4. **Python Example** (`examples/python-example/`): Uses flask, requests, numpy, and pytest
5. **C Example** (`examples/c-example/`): Uses openssl, libcurl, and zlib
6. **C++ Example** (`examples/cpp-example/`): Uses boost, fmt, nlohmann-json, and spdlog

#### Running Example Projects

Use the `just` command to run and test examples:

```sh
# Show available example commands
just examples

# Test Feluda on all example projects
just test-examples

# Test Feluda on a specific example
feluda --path examples/rust-example
feluda --path examples/node-example
feluda --path examples/go-example
feluda --path examples/python-example
feluda --path examples/c-example
feluda --path examples/cpp-example
```

#### Using Examples for Development

When developing new features or fixing bugs, use these example projects to:

1. **Test language-specific parsers**: Each example project tests a different language parser
2. **Verify transient dependency resolution**: All examples include dependencies with indirect dependencies
3. **Test license detection accuracy**: Examples use common libraries with well-known licenses
4. **Validate output formats**: Test JSON, YAML, verbose, and TUI modes on examples

Example workflow:

```sh
# Make your changes to the codebase
cargo build

# Test on all examples
just test-examples

# Test specific output formats
./target/debug/feluda --path examples/rust-example --json
./target/debug/feluda --path examples/node-example --verbose
./target/debug/feluda --path examples/go-example --gui
```

### Debug Mode

Feluda has a comprehensive debug system that helps with troubleshooting and development. To enable debug mode, run Feluda with the `--debug` or `-d` flag:

```sh
feluda --debug
```

#### Debug Features

The debug mode provides the following features:

1. **Detailed Logging**: Log messages are printed with different levels:
   - `INFO`: General information about operations
   - `WARN`: Potential issues that don't stop execution
   - `ERROR`: Problems that caused an operation to fail
   - `TRACE`: Detailed debugging information about data structures

2. **Performance Metrics**: Debug mode automatically times key operations and reports their duration.

3. **Data Inspection**: Complex data structures are printed in debug format for inspection.

4. **Error Context**: Errors include detailed context to help identify root causes.

#### Logging in Your Code

When adding new features, include appropriate logging using the debug module:

```rust
// Import debug utilities
use crate::debug::{log, LogLevel, log_debug, log_error};

// Log informational messages
log(LogLevel::Info, "Starting important operation");

// Log warnings
log(LogLevel::Warn, "Resource XYZ not found, using default");

// Log errors with context
if let Err(err) = some_operation() {
    log_error("Failed to complete operation", &err);
}

// Log complex data structures for debugging
log_debug("Retrieved configuration", &config);

// Time operations
let result = with_debug("Complex calculation", || {
    // Your code here
    perform_complex_calculation()
});
```

#### Error Handling

Feluda uses a custom error type for consistent error handling. When adding new code, use the `FeludaError` and `FeludaResult` types:

```rust
// Return a Result with a specific error type
fn my_function() -> FeludaResult<MyType> {
    match some_operation() {
        Ok(result) => Ok(result),
        Err(err) => Err(FeludaError::Parser(format!("Operation failed: {}", err)))
    }
}
```

#### Available Error Types

The `FeludaError` enum provides specific error variants for different error scenarios. Use the most specific error type that matches your situation:

| Error Variant | Use Case | Example |
|--------------|----------|---------|
| `Io(std::io::Error)` | File system operations, I/O errors | File read/write failures (auto-converted via `From` trait) |
| `Http(reqwest::Error)` | Network requests, API calls | HTTP client errors (auto-converted via `From` trait) |
| `Config(String)` | Configuration loading/validation | Invalid config values, missing required settings |
| `License(String)` | License analysis, compatibility checks | Invalid license format, compatibility violations |
| `Parser(String)` | Dependency file parsing | Malformed package.json, invalid Cargo.toml |
| `RepositoryClone(String)` | Git repository cloning | Clone failures, authentication issues |
| `TempDir(String)` | Temporary directory operations | Failed to create or access temp directories |
| `TuiInit(String)` | TUI initialization | Terminal setup failures, color_eyre errors |
| `TuiRuntime(String)` | TUI runtime operations | Runtime errors during TUI execution |
| `Serialization(String)` | JSON/YAML serialization | Failed to serialize SBOM documents |
| `FileWrite(String)` | File write operations | Failed to write SBOM or license files |
| `InvalidData(String)` | Data validation | Malformed SPDX data, invalid characters |
| `Unknown(String)` | Fallback for uncategorized errors | Use only when no specific type fits |

**Guidelines:**
- Prefer specific error types over `Unknown`
- Include context in error messages: `FeludaError::Parser(format!("Failed to parse {}: {}", file, err))`
- Use `map_err()` to convert errors: `.map_err(|e| FeludaError::Serialization(format!("Failed to serialize: {e}")))?`
- `Io` and `Http` errors are auto-converted via the `From` trait, no manual conversion needed

### Guidelines

- **Code Style**: Follow Rust's standard coding conventions.
- **Testing**: Ensure your changes are covered by unit tests.
- **Documentation**: Update relevant documentation and comments.
- **Logging**: Add appropriate debug logging for new functionality.
- **Error Handling**: Use the `FeludaError` type for consistent error reporting.

### Maintaining the License Compatibility Matrix

The license compatibility matrix is a critical component that determines which dependency licenses are compatible with different project licenses. This matrix is stored in `config/license_compatibility.toml` and requires careful maintenance.

#### Understanding the Matrix Structure

The compatibility matrix follows this TOML format:

```toml
[PROJECT_LICENSE_NAME]
compatible_with = [
    "dependency-license-1",
    "dependency-license-2",
    # ... more compatible licenses
]
```

Each section represents a project license, and the `compatible_with` array lists all dependency licenses that can be safely used with that project license.

#### Guidelines for Matrix Updates

**⚠️ Legal Expertise Required**: Modifying license compatibility rules requires legal knowledge. Consider these guidelines:

1. **Research Thoroughly**: 
   - Consult official license documentation
   - Review legal analyses from recognized authorities (FSF, OSI, etc.)
   - Check compatibility matrices from other trusted sources

2. **Conservative Approach**: 
   - When in doubt, mark as incompatible rather than compatible
   - Legal liability is better avoided than remedied later

3. **Common License Relationships**:
   - **Permissive → Permissive**: Generally compatible (MIT, BSD, Apache-2.0)
   - **Permissive → Copyleft**: Generally compatible (can include MIT in GPL project)
   - **Copyleft → Permissive**: Generally incompatible (cannot include GPL in MIT project)
   - **Copyleft → Same Copyleft**: Usually compatible (GPL-3.0 with GPL-3.0)
   - **Copyleft → Different Copyleft**: Requires careful analysis

4. **Testing Changes**:

```sh
# Test with the ignored license compatibility test
cargo test licenses::tests::test_is_license_compatible_mit_project -- --ignored

# Run all tests to ensure no regressions
cargo test

# Test with real projects to validate changes
feluda --project-license MIT --path /path/to/test/project
```

#### Adding New License Support

To add support for a new project license:

1. **Research the License**: Understand its permissions, conditions, and limitations
2. **Determine Compatibility**: Research which licenses are compatible
3. **Add to Matrix**: Add a new section in `config/license_compatibility.toml`:
   ```toml
   [NEW-LICENSE-1.0]
   compatible_with = [
       # List compatible dependency licenses based on legal research
   ]
   ```
4. **Update Normalization**: Add license variations to the `normalize_license_id` function in `src/licenses.rs`:
   ```rust
   id if id.contains("NEW-LICENSE") && id.contains("1.0") => "NEW-LICENSE-1.0".to_string(),
   ```
5. **Add Tests**: Include the new license in relevant test cases
6. **Document**: Update README.md to list the new supported license

#### Common License Compatibility Patterns

| Project License | Can Include Dependencies From |
|----------------|-------------------------------|
| **MIT/BSD/ISC** | Only permissive licenses (MIT, BSD, Apache, ISC, etc.) |
| **Apache-2.0** | Permissive licenses (same as MIT) |
| **GPL-3.0** | Most licenses (permissive + LGPL + GPL family) |  
| **GPL-2.0** | Permissive + LGPL + GPL-2.0 (NOT Apache-2.0) |
| **AGPL-3.0** | Similar to GPL-3.0 plus AGPL |
| **LGPL-3.0/2.1** | Limited compatibility (mainly permissive) |
| **MPL-2.0** | Permissive + MPL |

#### Review Process for Matrix Changes

All changes to the license compatibility matrix require:

1. **Detailed explanation** in the PR description of:
   - Why the change is needed
   - Legal reasoning or sources consulted
   - Impact on existing compatibility decisions

2. **Maintainer review** by someone with legal expertise or license knowledge

3. **Testing** with real-world projects to ensure changes work as expected

4. **Documentation updates** reflecting the changes

#### Legal Disclaimer

Contributors modifying the license compatibility matrix acknowledge that:
- This is not legal advice and should not be treated as such
- Users are responsible for their own license compliance
- Maintainers and contributors provide no warranty regarding compatibility decisions
- Legal counsel should be consulted for important compliance decisions

### OSI Integration

Feluda integrates with the Open Source Initiative (OSI) to provide license approval status information. The OSI integration consists of several components that work together to fetch, cache, and display OSI approval status for licenses.

#### OSI Integration Components

1. **OSI API Integration** (`src/licenses.rs`):
   - `fetch_osi_licenses()`: Fetches approved licenses from [OSI API](`https://api.opensource.org/licenses/`)
   - `OsiLicenseInfo` struct: Represents OSI license data structure
   - Concurrent HTTP requests with tokio for performance
   - Handles API failures gracefully with fallback mechanisms

2. **OSI Status Management**:
   - `OsiStatus` enum: `Approved`, `NotApproved`, `Unknown`
   - `get_osi_status()`: Maps SPDX license IDs to OSI approval status
   - Static fallback mappings for common licenses when API is unavailable
   - Integration in all language parsers to include OSI status in `LicenseInfo`

3. **Display Integration**:
   - OSI status column in verbose table mode (`src/table.rs`)
   - OSI status in JSON/YAML output formats (`src/reporter.rs`)
   - Color-coded OSI status display in TUI mode
   - CLI filtering with `--osi` flag (`src/cli.rs`)

#### Modifying OSI Integration

When working with OSI integration:

**Adding New Static Mappings**: Update `get_osi_status()` in `src/licenses.rs`:
```rust
pub fn get_osi_status(license_id: &str, osi_licenses: &[OsiLicenseInfo]) -> OsiStatus {
    // Add new static mappings here for licenses not in OSI API
    match license_id {
        "NEW-LICENSE-ID" => OsiStatus::Approved, // If you know it's OSI approved
        // ... existing mappings
    }
}
```

**Testing OSI Integration**:
```sh
# Test OSI API connectivity and filtering
cargo run -- --osi approved --verbose

# Test fallback behavior (when API fails)
# Temporarily break API URL in code and test

# Test JSON output includes osi_status field
cargo run -- --json | jq '.[0].osi_status'
```

### Adding Support for New Languages

Feluda follows a modular architecture for language support. Each language has its own module in `src/languages/` that implements dependency parsing and license resolution.

#### Language Module Structure

When adding a new language, create a new file `src/languages/your_language.rs` with this structure:

```rust
use crate::config::FeludaConfig;
use crate::debug::{log, LogLevel};
use crate::licenses::LicenseInfo;
use std::collections::{HashMap, HashSet};

/// Analyze dependencies and their licenses for YourLanguage projects
pub fn analyze_your_language_licenses(project_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
    // Implementation here
}

/// Parse direct dependencies from project files
fn parse_direct_dependencies(project_path: &str, config: &FeludaConfig) -> Vec<(String, String)> {
    // Parse project files (package.json, Cargo.toml, etc.)
}

/// Resolve transitive dependencies with depth tracking
fn resolve_transitive_dependencies(
    project_path: &str,
    direct_deps: &[(String, String)],
    max_depth: u32,
) -> Vec<(String, String)> {
    // Implement BFS traversal for transitive dependencies
    // Follow the pattern used in other language modules
}

/// Fetch license information for a specific dependency
fn fetch_license_for_dependency(name: &str, version: &str) -> Option<String> {
    // Query package registries/APIs for license information
}
```

#### Implementation Guidelines for Language Modules

1. **Follow Existing Patterns**: Study `src/languages/rust.rs` or `src/languages/python.rs` for reference implementation patterns.

2. **Transitive Dependency Resolution**: Implement BFS (Breadth-First Search) traversal with these features:
   - Cycle detection using `HashSet<String>` to track visited packages
   - Depth tracking with `max_depth` parameter from config
   - Progress tracking for large dependency trees

3. **Error Handling**: Use the debug logging system consistently:
   ```rust
   use crate::debug::{log, LogLevel};

   if let Err(err) = some_operation() {
       log(LogLevel::Warn, &format!("Failed to fetch {}: {}", package_name, err));
   }
   ```

4. **Configuration Support**: Respect the `max_depth` configuration:
   ```rust
   let max_depth = config.max_depth.unwrap_or(3);
   ```

5. **Package Manager Integration**: Connect to official package registries when possible:
   - Query official APIs for license information
   - Handle API rate limits and failures gracefully
   - Cache results when appropriate

#### C/C++ Language Implementation Example

The C and C++ modules (`src/languages/c.rs` and `src/languages/cpp.rs`) demonstrate handling different ecosystem approaches:

**C Module Features:**
- Autotools support (`configure.ac`, `configure.in`)
- Makefile parsing
- pkg-config integration
- System package resolution

**C++ Module Features:**
- Modern package managers (vcpkg, Conan)
- Build system integration (CMake, Bazel)
- Package manager API queries
- Transitive dependency resolution

#### Updating Language Detection

After creating your language module, update `src/languages/mod.rs`:

1. **Add the module**:
   ```rust
   pub mod your_language;
   ```

2. **Add to exports**:
   ```rust
   pub use your_language::analyze_your_language_licenses;
   ```

3. **Update Language enum**:
   ```rust
   pub enum Language {
       YourLanguage(&'static str),
       // ... existing variants
   }
   ```

4. **Add file detection**:
   ```rust
   impl Language {
       pub fn from_file_name(file_name: &str) -> Option<Self> {
           match file_name {
               "your-project-file.ext" => Some(Language::YourLanguage("your-project-file.ext")),
               // ... existing patterns
           }
       }
   }
   ```

5. **Update parser.rs**: Add parsing logic in `src/parser.rs`:
   ```rust
   match language {
       Language::YourLanguage(file_name) => {
           languages::analyze_your_language_licenses(project_path, config)
       }
       // ... existing cases
   }
   ```

#### Testing New Language Support

1. **Create test projects** in various scenarios
2. **Test transitive dependency resolution** with different depth configurations
3. **Validate license detection** accuracy
4. **Test error handling** for invalid/missing project files

#### Documentation Updates

After implementing language support:

1. **Update README.md** with language badge and supported file types
2. **Add usage examples** specific to your language
3. **Document supported package managers** and build systems
4. **Update CLI help text** to include the new language filter

### Submitting Changes

1. Create a new branch for your feature or bugfix:

```sh
git checkout -b feat/my-new-feature
```

2. Commit your changes with a meaningful commit message:

```sh
git commit -m "Add support for XYZ feature"
```

3. Push the branch to your fork:

```sh
git push origin feat/my-new-feature
```

4. Open a pull request on GitHub.

### Reporting Issues

If you encounter a bug or have a feature request, please open an issue in the repository.

### Releasing Feluda 🚀

This is only if you've release permissions. If not, contact the maintainers to get it.

#### Setup Requirements

- Install the gh CLI:
```sh
brew install gh # macOS
sudo apt install gh # Ubuntu/Debian
```

- Authenticate the gh CLI with GitHub:
```sh
gh auth login
```

- Install jq for JSON parsing:
```sh
brew install jq # macOS
sudo apt install jq # Ubuntu/Debian
```

We'll be using justfile for next steps, so setup [just](https://github.com/casey/just) before proceeding...

#### Build the Release
```sh
just release
```

#### Test the Release Build
```sh
just test-release
```

#### Create the Package
Validate the crate before publishing
```sh
just package
```

#### Publish the Crate
```sh
just publish
```

#### Automate Everything
Run all steps (build, test, package, and publish) in one command:

```sh
just release-publish
```

#### Clean Artifacts
To clean up the build artifacts:

```sh
just clean
```

#### Login to crates.io
```sh
just login
```
