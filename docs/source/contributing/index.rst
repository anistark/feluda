:description: Guide to contributing to Feluda development.

.. _contributing:

Contributing
============

.. rst-class:: lead

   Welcoming contributions from the community!

----

**Minimum Supported Rust Version:** ``1.85.0``

**Currently Working Rust Version:** ``1.88.0``

Folder Structure
----------------

.. code-block:: text

   feluda/
   ├── src/
   │   ├── main.rs              # Entry point
   │   ├── cli.rs               # CLI argument handling
   │   ├── cache.rs             # Caching functionality for license data
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
   │       ├── python.rs        # Python package support
   │       └── r.rs             # R package support
   ├── examples/                # Example projects for testing
   ├── config/
   │   └── license_compatibility.toml  # License compatibility matrix
   ├── Cargo.toml               # Project metadata
   ├── LICENSE                  # Project license
   ├── README.md                # Project documentation
   ├── justfile                 # Build automation
   └── CONTRIBUTING.md

Guidelines
----------

- **Code Style**: Follow Rust's standard coding conventions.
- **Testing**: Ensure your changes are covered by unit tests.
- **Documentation**: Update relevant documentation and comments.
- **Logging**: Add appropriate debug logging for new functionality.
- **Error Handling**: Use the ``FeludaError`` type for consistent error reporting.

Submitting Changes
------------------

1. Create a new branch for your feature or bugfix:

   .. code-block:: sh

      git checkout -b feat/my-new-feature

2. Commit your changes with a meaningful commit message:

   .. code-block:: sh

      git commit -m "Add support for XYZ feature"

3. Push the branch to your fork:

   .. code-block:: sh

      git push origin feat/my-new-feature

4. Open a pull request on GitHub.

Reporting Issues
----------------

If you encounter a bug or have a feature request, please open an issue in the repository.
