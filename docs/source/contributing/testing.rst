:description: Testing Feluda with example projects and debug mode.

Testing
=======

.. rst-class:: lead

   Test your changes using example projects and debug mode.

----

Testing with Example Projects
-----------------------------

Feluda includes example projects for all supported languages in the ``examples/`` directory. These projects are designed to test Feluda's license analysis capabilities with real-world dependencies that have transient (indirect) dependencies.

Available Example Projects
^^^^^^^^^^^^^^^^^^^^^^^^^^

1. **Rust Example** (``examples/rust-example/``): Uses serde, tokio, reqwest, and clap
2. **Node.js Example** (``examples/node-example/``): Uses express, axios, lodash, and moment
3. **Go Example** (``examples/go-example/``): Uses gin, cobra, testify, and zap
4. **Python Example** (``examples/python-example/``): Uses flask, requests, numpy, and pytest
5. **C Example** (``examples/c-example/``): Uses openssl, libcurl, and zlib
6. **C++ Example** (``examples/cpp-example/``): Uses boost, fmt, nlohmann-json, and spdlog
7. **R Example** (``examples/r-example/``): Uses dplyr, ggplot2, and tidyr

Running Example Projects
^^^^^^^^^^^^^^^^^^^^^^^^

Use the ``just`` command to run and test examples:

.. code-block:: sh

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
   feluda --path examples/r-example

   # Test SBOM generation on an example project
   feluda sbom --path examples/rust-example
   feluda sbom spdx --path examples/rust-example
   feluda sbom cyclonedx --path examples/rust-example

Using Examples for Development
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

When developing new features or fixing bugs, use these example projects to:

1. **Test language-specific parsers**: Each example project tests a different language parser
2. **Verify transient dependency resolution**: All examples include dependencies with indirect dependencies
3. **Test license detection accuracy**: Examples use common libraries with well-known licenses
4. **Validate output formats**: Test JSON, YAML, verbose, and TUI modes on examples

Example workflow:

.. code-block:: sh

   # Make your changes to the codebase
   cargo build

   # Test on all examples
   just test-examples

   # Test specific output formats
   ./target/debug/feluda --path examples/rust-example --json
   ./target/debug/feluda --path examples/node-example --verbose
   ./target/debug/feluda --path examples/go-example --gui

   # Test SBOM generation
   ./target/debug/feluda sbom --path examples/rust-example
   ./target/debug/feluda sbom spdx --path examples/rust-example
   ./target/debug/feluda sbom cyclonedx --path examples/node-example

Debug Mode
----------

Feluda has a comprehensive debug system that helps with troubleshooting and development. To enable debug mode, run Feluda with the ``--debug`` or ``-d`` flag:

.. code-block:: sh

   feluda --debug

Debug Features
^^^^^^^^^^^^^^

The debug mode provides the following features:

1. **Detailed Logging**: Log messages are printed with different levels:

   - ``INFO``: General information about operations
   - ``WARN``: Potential issues that don't stop execution
   - ``ERROR``: Problems that caused an operation to fail
   - ``TRACE``: Detailed debugging information about data structures

2. **Performance Metrics**: Debug mode automatically times key operations and reports their duration.

3. **Data Inspection**: Complex data structures are printed in debug format for inspection.

4. **Error Context**: Errors include detailed context to help identify root causes.

Logging in Your Code
^^^^^^^^^^^^^^^^^^^^

When adding new features, include appropriate logging using the debug module:

.. code-block:: rust

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
