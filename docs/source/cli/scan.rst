:description: Feluda scan command for detecting licenses in dependencies.

.. _cli-scan:

scan
====

.. rst-class:: lead

   The primary command for scanning dependencies and detecting licenses.

----

Basic Scan
----------

Feluda defaults to scanning the working directory, which is perfect for quick health checks before pushing code.

.. code-block:: bash

   feluda

Feluda prints a table describing each dependency's license, OSI status, restrictive flag, and compatibility hints.

----

Scan a Specific Path
--------------------

Point Feluda at another workspace without changing directories when you're triaging multiple repositories.

.. code-block:: bash

   feluda --path /path/to/project

Feluda walks the supplied directory recursively and reports results just like the default scan.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--path <PATH>``
     - Absolute or relative path to scan

----

Scan a Remote Repository
------------------------

Feluda can clone and inspect remote codebases, which keeps auditors away from messy temporary checkouts.

.. code-block:: bash

   feluda --repo <repository_url>

Feluda clones the repository into a temporary location, performs the scan, and removes the clone after inspection.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--repo <URL>``
     - Git repository URL (SSH or HTTPS)
   * - ``--ssh-key <PATH>``
     - Path to SSH private key for authentication
   * - ``--ssh-passphrase <PASS>``
     - Passphrase for the SSH key
   * - ``--token <TOKEN>``
     - HTTPS token for private repositories

**Example with SSH:**

.. code-block:: bash

   feluda --repo git@github.com:org/repo.git --ssh-key ~/.ssh/id_ed25519 --ssh-passphrase "passphrase"

**Example with HTTPS:**

.. code-block:: bash

   feluda --repo https://github.com/org/repo.git --token "ghp_xxxx"

.. note::
   Provide only the secrets you truly need; Feluda happily works with HTTPS tokens, SSH keys, or neither for public repos.

----

Scan a Workspace or Monorepo
----------------------------

Feluda automatically detects multi-package projects and produces one unified
report covering every sub-project, so you don't have to run separate scans.

**Cargo workspaces** — point Feluda at the workspace root (the directory
containing the ``[workspace]`` ``Cargo.toml``). Workspace members themselves
are excluded from the report; their transitive dependencies are attributed to
the member that pulls them in.

**npm/yarn/pnpm workspaces** — point Feluda at the directory containing the
root ``package.json`` with the ``workspaces`` field. Each dependency is
attributed to the workspace package(s) that declare it.

**Go workspaces** — point Feluda at the directory containing ``go.work``.
Feluda parses the ``use`` directives, scans each member module, and merges
the results.

**Python uv workspaces** — point Feluda at the directory containing the root
``pyproject.toml`` with a ``[tool.uv.workspace]`` section. Feluda expands the
``members`` glob list (honoring ``exclude``), reads each member's
``pyproject.toml``, and attributes every declared dependency to its member.

.. code-block:: bash

   feluda --path /path/to/monorepo

In a monorepo scan, Feluda's standard table grows a *Workspace breakdown*
section listing the dependency count per sub-project:

.. code-block:: text

   📦 Total dependencies scanned: 142

   🧩 Workspace breakdown:
     • 98 api
     • 76 worker
     • 42 cli

In ``--verbose`` mode, the table also gains a **Sub-project** column showing
which workspace member(s) pulled in each dependency. Shared dependencies list
all owners separated by commas.

In ``--json`` and ``--yaml`` output, every dependency carries an optional
``sub_project`` field. The field is omitted entirely for non-workspace scans
to keep the schema stable.

----

Control Local vs Remote Detection
---------------------------------

Feluda defaults to examining local manifest files (``Cargo.toml``, ``node_modules``) before calling APIs; sometimes you want the opposite.

.. code-block:: bash

   feluda --no-local

Feluda skips filesystem lookups and goes straight to remote sources, which is slower but ensures up-to-date results.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--no-local``
     - Force remote-only detection from GitHub or registries

----

Authenticate with GitHub
------------------------

Large scans burn through unauthenticated rate limits quickly.

**Inline token:**

.. code-block:: bash

   feluda --github-token <your_token>

Feluda uses the supplied token for that invocation only.

**Environment variable:**

.. code-block:: bash

   export GITHUB_TOKEN=<your_token>

Feluda automatically picks up the variable, so every subsequent command benefits from 5,000 requests/hour.

.. important::
   The token only needs ``repo`` scope for private repos; public projects work with default scopes.

----

Fail CI Early
-------------

CI builds should stop the moment restrictive or incompatible dependencies sneak in.

**Fail on restrictive licenses:**

.. code-block:: bash

   feluda --fail-on-restrictive

Feluda returns a failure when it sees entries from the restrictive list, making CI pipelines halt.

**Fail on incompatible licenses:**

.. code-block:: bash

   feluda --fail-on-incompatible

Feluda exits with failure if any dependency violates the compatibility matrix.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Flag
     - Description
   * - ``--fail-on-restrictive``
     - Exit non-zero when restrictive licenses are found
   * - ``--fail-on-incompatible``
     - Exit non-zero when incompatible licenses are found
