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
│   ├── licenses.rs          # License analysis and compatibility
│   ├── reporter.rs          # Output formatting and reporting
│   ├── table.rs             # TUI components
│   ├── generate.rs          # License file generation
│   ├── utils.rs             # Git repository utilities
│   └── languages/           # Language-specific parsers
│       ├── mod.rs           # Language detection and common traits
│       ├── rust.rs          # Rust/Cargo support
│       ├── node.rs          # Node.js/npm support
│       ├── go.rs            # Go modules support
│       └── python.rs        # Python package support
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
