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
5. **Java/Maven Example** (``examples/java-example/maven-example/``): Uses guava, commons-lang3, jackson-databind, slf4j, logback, and spring-core
6. **Java/Gradle Example** (``examples/java-example/gradle-example/``): Uses guava, commons-lang3, jackson-databind, slf4j, logback, httpclient5, spring-boot-starter, and h2
7. **C Example** (``examples/c-example/``): Uses openssl, libcurl, and zlib
8. **C++ Example** (``examples/cpp-example/``): Uses boost, fmt, nlohmann-json, and spdlog
9. **R Example** (``examples/r-example/``): Uses dplyr, ggplot2, and tidyr

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
   feluda --path examples/java-example/maven-example
   feluda --path examples/java-example/gradle-example
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

Counting Network Requests
^^^^^^^^^^^^^^^^^^^^^^^^^

Set ``FELUDA_REQUEST_STATS`` to a file path and Feluda writes a per host tally of every HTTP request it sent when the run ends: how many, which statuses came back, how many failed before any status, how many were never sent because the host had already been given up on, and the total and slowest time to the response headers.

.. code-block:: sh

   FELUDA_REQUEST_STATS=stats.json feluda --path examples/python-example

.. code-block:: json

   {
     "total_requests": 33,
     "hosts": {
       "pypi.org": {
         "requests": 18,
         "transport_errors": 0,
         "skipped": 0,
         "statuses": { "200": 18 },
         "total_ms": 9349.2,
         "max_ms": 1104.7
       }
     }
   }

Requests run concurrently, so a host's ``total_ms`` can be longer than the run itself. A new request site goes through ``http_stats::get``, ``http_stats::send`` or ``http_stats::send_async`` in ``src/http_stats.rs`` so it is counted and its host can be given up on; a client of its own starts from ``http_stats::client_builder`` (or ``async_client_builder``) so it gets the configured timeouts.

Benchmarks
^^^^^^^^^^

The numbers on :doc:`../performance` come from the harness in ``bench/``: scripts that time a release build on a synthetic ladder and on pinned real repositories, with every cache state set per run, and that generate the page from the results. ``just bench-page`` rebuilds the page from the committed results without touching the network, ``just bench-cell <subject> <scenario>`` runs one cell and ``just bench`` runs the whole study; ``bench/README.md`` covers setup and the rest. The page's prose lives in ``bench/report/performance.rst.tmpl``; edit that, not ``performance.rst``.
