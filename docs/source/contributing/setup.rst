:description: Setting up Feluda for local development.

Development Setup
=================

.. rst-class:: lead

   Get your development environment ready to contribute to Feluda.

----

Setting Up for Development
--------------------------

1. Fork the repository and clone it to your local machine:

   .. code-block:: sh

      git clone https://github.com/yourusername/feluda.git
      cd feluda

2. Install dependencies and tools:

   .. code-block:: sh

      cargo build

3. Configure git hooks (recommended for automatic checks on commit):

   .. code-block:: sh

      # Setup development environment with pre-commit hooks
      just setup

   This will:

   - Make hook scripts executable
   - Configure git to use ``.githooks`` directory
   - Enable automatic pre-commit checks

4. Run locally:

   .. code-block:: sh

      ./target/debug/feluda --help

5. Run tests to ensure everything is working:

   .. code-block:: sh

      cargo test

6. Install feluda system-wide (optional):

   .. code-block:: sh

      just install

   This will:

   - Build the release binary with all checks
   - Copy the binary to ``/usr/local/bin/feluda`` for system-wide access
   - Make ``feluda`` available globally in your terminal

   Verify the installation:

   .. code-block:: sh

      feluda --version

Pre-commit Hooks
----------------

The repository includes automated pre-commit hooks that run:

- Format checks (``cargo fmt --all -- --check``)
- Linting (``cargo clippy --all-targets --all-features -- -D warnings``)
- All tests (``cargo test``)

**Setup options:**

Option A: Git hooks (automatic) - Recommended
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

.. code-block:: sh

   # After cloning, run once:
   just setup

   # Now CI checks will run automatically on every commit
   git commit -m "your message"

If you prefer manual setup:

.. code-block:: sh

   git config core.hooksPath .githooks
   chmod +x .githooks/*

Option B: Using pre-commit framework
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

.. code-block:: sh

   # Install pre-commit if you don't have it
   pip install pre-commit

   # Install the hooks
   pre-commit install

   # CI checks will run automatically on every commit
   git commit -m "your message"

Option C: Run CI checks manually
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

.. code-block:: sh

   just test-ci

This runs all checks exactly as the GitHub Actions CI does, without committing.

Documentation
-------------

To work on the documentation locally:

.. code-block:: sh

   # Install documentation dependencies (first time only)
   just docs-setup

   # Serve docs locally with live reload
   just docs-serve

   # Build documentation
   just docs-build

   # Clean documentation build artifacts
   just docs-clean
