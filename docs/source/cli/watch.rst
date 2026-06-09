:description: Feluda watch command for continuous license scanning.

.. _cli-watch:

watch
=====

.. rst-class:: lead

   Keep Feluda on the case continuously — re-scan automatically whenever a dependency file changes.

----

Overview
--------

``feluda watch`` monitors your project tree for filesystem changes and re-runs the
license scan whenever a dependency manifest or lockfile is added, modified, or
removed. It is handy while adding or upgrading dependencies — especially with AI
coding tools that pull in packages on the fly — so license issues surface the
moment they appear.

Watch mode is **report-only**: it prints the same report as a normal scan on every
change and keeps running until interrupted with ``Ctrl-C``.

----

Basic Usage
-----------

.. code-block:: bash

   # Watch the current directory
   feluda watch

   # Watch a specific path
   feluda watch --path /path/to/project/

   # Adjust the debounce window (milliseconds)
   feluda watch --debounce 800

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--path``, ``-p``
     - Project directory to watch (default: ``./``)
   * - ``--debounce``
     - Milliseconds to wait after a change before re-scanning (default: ``500``)

----

Reusing Scan Flags
------------------

Watch mode inherits the same output and filter flags as a normal scan. Pass them
**before** the ``watch`` subcommand:

.. code-block:: bash

   # Watch and report only restrictive licenses, as JSON
   feluda --restrictive --json watch

   # Watch in strict mode against a specific project license
   feluda --strict --project-license MIT watch

----

What Feluda Watches
-------------------

Feluda recursively discovers every dependency manifest and lockfile it understands,
honouring ``.gitignore``/``.ignore`` and skipping vendored directories such as
``node_modules/``, ``target/``, ``vendor/``, and ``.git/``.

This is the same set of files the scanner recognises, including:

- **Rust** — ``Cargo.toml``, ``Cargo.lock``
- **Node.js** — ``package.json``, ``package-lock.json``, ``yarn.lock``, ``pnpm-lock.yaml``
- **Go** — ``go.mod``, ``go.work``, ``go.sum``, ``go.work.sum``
- **Python** — ``pyproject.toml``, ``requirements.txt``, ``Pipfile.lock``, ``uv.lock``
- **Java** — ``pom.xml``, ``build.gradle``, ``build.gradle.kts``
- **.NET** — ``*.csproj``, ``*.fsproj``, ``*.vbproj``, ``*.slnx``, ``packages.lock.json``
- **C/C++** — ``vcpkg.json``, ``conanfile.txt``, ``CMakeLists.txt``, ``MODULE.bazel``
- **R** — ``DESCRIPTION``, ``renv.lock``

.. note::
   Watch mode does not support the interactive TUI (``--gui``) or remote
   repositories (``--repo``). Use a local path and a non-interactive report.

----

Scheduled & Looped Scanning
---------------------------

When a long-running watcher isn't a good fit — CI runners, network filesystems, or
periodic compliance audits — run Feluda's single-shot scan on a schedule instead.
Combine it with ``--fail-on-restrictive`` (or ``--fail-on-incompatible``) so the
exit code gates your pipeline.

.. code-block:: bash

   # cron — scan every 30 minutes, fail on a restrictive license
   */30 * * * * cd /path/to/project && feluda --fail-on-restrictive

   # Claude Code /loop — re-run the check every 30 minutes
   /loop 30m feluda --restrictive

   # CI gate — non-zero exit stops the build
   feluda --fail-on-restrictive --json

.. tip::
   Use ``feluda watch`` for fast feedback during local development, and the
   scheduled single-shot scan above for unattended or CI environments.
