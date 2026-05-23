:description: Use the Feluda Claude Code skill for inline license compliance while coding.

.. _claude-code:

Claude Code
===========

.. rst-class:: lead

   The Feluda Claude Code skill watches your dependency changes in real time and surfaces license warnings before a single line gets committed.

----

Overview
--------

The Feluda skill integrates directly into `Claude Code <https://claude.ai/code>`_, Anthropic's AI coding assistant. Once installed, Claude automatically runs ``feluda`` whenever it detects changes to a dependency manifest — flagging GPL, AGPL, SSPL, and other restrictive licenses inline in your session, before you commit.

It also responds to natural-language requests: "check my licenses", "is this package safe to use?", or "audit my dependencies".

----

Installation
------------

The skill is published on the `AgentHub marketplace <https://github.com/nullorder/agenthub>`_.

**Install from AgentHub (recommended):**

.. code-block:: text

   /plugin install feluda@agenthub

**Install directly from source:**

.. code-block:: text

   /plugin install git+https://github.com/anistark/feluda?path=skills/feluda

After installing, Claude Code loads the skill automatically — no restart needed.

Prerequisites
^^^^^^^^^^^^^

The skill requires the ``feluda`` CLI on your ``PATH``. Install it once:

.. code-block:: bash

   # via Rust (any OS)
   cargo install feluda

   # via Homebrew (macOS / Linux)
   brew install feluda

Binaries for Linux, macOS, and Windows are also available on the
`GitHub Releases <https://github.com/anistark/feluda/releases>`_ page.

----

Usage
-----

Automatic triggering
^^^^^^^^^^^^^^^^^^^^

The skill activates automatically when Claude Code sees a dependency manifest in the
current diff or staged changes. Supported files include:

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Ecosystem
     - Files
   * - Rust
     - ``Cargo.toml``, ``Cargo.lock``
   * - Node.js
     - ``package.json``, ``package-lock.json``, ``yarn.lock``, ``pnpm-lock.yaml``, ``bun.lockb``
   * - Go
     - ``go.mod``, ``go.sum``
   * - Python
     - ``requirements.txt``, ``pyproject.toml``, ``Pipfile.lock``, ``pip_freeze.txt``
   * - Java
     - ``pom.xml``, ``build.gradle``, ``build.gradle.kts``
   * - .NET
     - ``*.csproj``, ``*.fsproj``, ``*.vbproj``, ``*.slnx``
   * - Ruby
     - ``Gemfile``, ``Gemfile.lock``
   * - PHP
     - ``composer.json``, ``composer.lock``
   * - C / C++
     - ``vcpkg.json``, ``conanfile.txt``, ``CMakeLists.txt``
   * - R
     - ``DESCRIPTION``, ``renv.lock``

Manual invocation
^^^^^^^^^^^^^^^^^

Trigger the skill explicitly with the slash command:

.. code-block:: text

   /feluda

Or ask naturally inside any Claude Code session:

.. code-block:: text

   Check my licenses before I commit
   Is this npm package safe to use?
   Audit my dependencies
   Does this project have any GPL dependencies?

----

Scan Options
------------

The skill passes flags to ``feluda`` based on context. You can also ask for a
specific scan mode:

.. list-table::
   :header-rows: 1
   :widths: 40 60

   * - Ask Claude / intent
     - Underlying command
   * - Default pre-commit check
     - ``feluda --restrictive``
   * - Full audit of all licenses
     - ``feluda --verbose``
   * - Only incompatible licenses
     - ``feluda --incompatible``
   * - Check against a specific license
     - ``feluda --project-license MIT``
   * - Scan a subdirectory (monorepo)
     - ``feluda --path ./packages/my-lib``
   * - Machine-readable output
     - ``feluda --json``

For a complete reference of all CLI flags see :doc:`/cli/scan` and :doc:`/cli/filter`.

----

Reading the Results
-------------------

**No issues found:**

The skill confirms the scan passed and continues without interruption.

**Restrictive licenses found:**

Claude surfaces a summary table with the flagged packages, their license identifiers,
and why each is a concern:

.. code-block:: text

   ⚠ feluda found license issues:

   Package           License    Why it matters
   ──────────────────────────────────────────────────────────────────
   some-lib@1.2.0    GPL-3.0    Copyleft — distribution requires source disclosure
   other-lib@3.0.0   AGPL-3.0   Network copyleft — SaaS use triggers source requirement

   Options:
   1. Find an MIT/Apache-2.0 alternative
   2. Add to .feluda.toml under [licenses] ignore = [...] if intentional
   3. Run `feluda --verbose` for full compatibility details

**Unknown licenses:**

When feluda cannot resolve a license (usually due to GitHub API rate limits), Claude
suggests setting a ``GITHUB_TOKEN`` in the environment to increase the limit from
60 to 5,000 requests per hour.

----

Pre-commit Enforcement
----------------------

To enforce compliance on every ``git commit`` — not just inside Claude Code — run:

.. code-block:: bash

   feluda init

This generates a ``.feluda.toml`` project config and adds a ``feluda`` hook to
``.pre-commit-config.yaml``. After activation:

.. code-block:: bash

   pre-commit install

Every ``git commit`` will now run ``feluda --fail-on-restrictive`` automatically,
refusing the commit if restrictive licenses are present.

See :doc:`/cli/scan` for the full list of ``feluda init`` options.

----

Configuration
-------------

Create a ``.feluda.toml`` in your project root to customise the skill's behaviour:

.. code-block:: toml

   [licenses]
   project = "MIT"          # your project's own license
   ignore = ["BSD-2-Clause"] # mark specific licenses as acceptable

   [packages]
   ignore = ["some-dual-licensed-lib"] # skip specific packages

When a ``.feluda.toml`` is present, the skill respects it automatically — no extra
flags needed.

Full configuration reference: :doc:`/configuration`.

----

GitHub Token
------------

For projects with many dependencies, set a ``GITHUB_TOKEN`` to raise the API rate
limit from 60 to 5,000 requests per hour:

.. code-block:: bash

   export GITHUB_TOKEN=ghp_...

Or add it to ``.feluda.toml``:

.. code-block:: toml

   [api]
   github_token = "ghp_..."

.. warning::

   Never commit a ``.feluda.toml`` containing a real token. Use an environment
   variable or a secrets manager instead.
