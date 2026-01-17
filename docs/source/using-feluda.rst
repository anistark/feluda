:description: Step-by-step workflows for scanning, filtering, and interpreting Feluda results.
.. _using-feluda:

Using Feluda
============

.. rst-class:: lead

   Put Feluda on the case with deliberate commands, filters, and output modes tailored to each investigation.

----

Run your first scan
-------------------

Feluda defaults to scanning the working directory, which is perfect for quick health checks before pushing code.

Run this when you want Feluda to inspect every supported ecosystem in the current directory.

.. code-block:: bash

   feluda

Feluda prints a table describing each dependency’s license, OSI status, restrictive flag, and compatibility hints.

----

Scan a specific path
--------------------

Point Feluda at another workspace without changing directories when you’re triaging multiple repositories.

Use this command when you want to scan an absolute or relative path.

.. code-block:: bash

   feluda --path /path/to/project

Feluda walks the supplied directory recursively and reports results just like the default scan.

----

Scan a remote repository
------------------------

Feluda can clone and inspect remote codebases, which keeps auditors away from messy temporary checkouts.

Trigger this command when you need to investigate a Git URL with optional SSH or HTTPS credentials.

.. code-block:: bash

   feluda --repo <repository_url> --ssh-key ~/.ssh/id_ed25519 --ssh-passphrase "<passphrase>" --token "<https_token>"

Feluda clones the repository into a temporary location, performs the scan, and removes the clone after inspection.

.. note::
   Provide only the secrets you truly need; Feluda happily works with HTTPS tokens, SSH keys, or neither for public repos.

----

Filter by language
------------------

Mixed-language monorepos can overwhelm you with noise, so narrow the scope to the stack you’re reviewing.

Run this when you want Feluda to focus on one ecosystem such as Rust, Node, Go, Python, C, C++, or R.

.. code-block:: bash

   feluda --language rust

Feluda shows entries only for the selected language while leaving the rest untouched for future scans.

----

Filter by OSI status
--------------------

Compliance teams often care about whether a license is OSI approved, unknown, or unapproved.

Use this command when you want only OSI-approved licenses in your report.

.. code-block:: bash

   feluda --osi approved

Feluda trims the output to dependencies whose licenses the OSI has blessed.

Run this option when you prefer to spotlight potential problem areas.

.. code-block:: bash

   feluda --osi not-approved

Feluda now highlights the packages that lack OSI approval, helping you escalate early.

Call this mode when you want to isolate dependencies whose OSI status the registry cannot confirm.

.. code-block:: bash

   feluda --osi unknown

Feluda prints only the entries with unknown OSI status so you can investigate manually.

----

Spot restrictive or incompatible licenses
-----------------------------------------

Feluda color-codes risk, but sometimes you just want the risky findings.

Use this command when only restrictive licenses (GPL-3.0, AGPL-3.0, MPL-2.0, etc.) matter for the moment.

.. code-block:: bash

   feluda --restrictive

Feluda lists only the dependencies carrying licenses from your restrictive list or config.

Run this when you want to see every dependency that conflicts with your declared project license.

.. code-block:: bash

   feluda --incompatible

Feluda now filters to dependencies whose licenses fail the compatibility matrix described in :ref:`reference`.

Call this when you want Feluda to check a specific outbound license before redistribution.

.. code-block:: bash

   feluda --project-license MIT

Feluda compares every dependency against the MIT row in ``config/license_compatibility.toml`` and flags conflicts.

----

Fail CI early
-------------

CI builds should stop the moment restrictive or incompatible dependencies sneak in.

Run this when you need Feluda to exit with a non-zero status on restrictive licenses.

.. code-block:: bash

   feluda --fail-on-restrictive

Feluda now returns a failure when it sees entries from the restrictive list, making CI pipelines halt.

Use this flag when incompatible licenses must also stop the build.

.. code-block:: bash

   feluda --fail-on-incompatible

Feluda exits with failure if any dependency violates the compatibility matrix, mirroring :ref:`automate-integrate` guidance.

----

Control local vs remote detection
---------------------------------

Feluda defaults to examining local manifest files (``Cargo.toml``, ``node_modules``) before calling APIs; sometimes you want the opposite.

Run this when you need to force freshly fetched license metadata from GitHub or registries.

.. code-block:: bash

   feluda --no-local

Feluda skips filesystem lookups and goes straight to remote sources, which is slower but ensures up-to-date results.

----

Manage the cache
----------------

Feluda caches GitHub license responses under ``.feluda/cache/github_licenses.json`` to stay under rate limits.

Use this command when you want to inspect cache statistics like size and freshness.

.. code-block:: bash

   feluda cache

Feluda prints the cache location, the number of entries, and whether the cache is still valid.

Run this when you need to clear stale or corrupted cache data.

.. code-block:: bash

   feluda cache --clear

Feluda deletes the cache file so the next scan starts fresh with remote data.

.. tip::
   Cache files older than 30 days refresh automatically, but explicit clears help when switching GitHub identities.

----

Authenticate with GitHub
------------------------

Large scans burn through unauthenticated rate limits quickly.

Use this argument when you want to pass a token inline (ideal for one-off runs).

.. code-block:: bash

   feluda --github-token <your_token>

Feluda uses the supplied token for that invocation only.

Export the environment variable when you prefer to configure CI or your shell once.

.. code-block:: bash

   export GITHUB_TOKEN=<your_token>

Feluda automatically picks up the variable, so every subsequent command benefits from 5,000 requests/hour.

.. important::
   The token only needs ``repo`` scope for private repos; public projects work with default scopes.

----

Enter GUI or verbose mode
-------------------------

Sometimes you want a richer view without leaving the terminal’s comfort.

Run this when you want to browse dependencies in Feluda’s TUI.

.. code-block:: bash

   feluda --gui

Feluda launches the graphical interface, letting you scroll through dependencies with OSI and compatibility badges.

Use verbose mode when you prefer extra columns (including OSI) in the standard output.

.. code-block:: bash

   feluda --verbose

Feluda adds OSI status and extended descriptions to the CLI table.

----

Interpret output formats
------------------------

Different consumers prefer different shapes of the same evidence.

Run this when you need machine-readable JSON for downstream automation.

.. code-block:: bash

   feluda --json

Feluda emits a JSON array containing dependency names, versions, licenses, restriction flags, and OSI status.

Call this when YAML integrates better with configuration management.

.. code-block:: bash

   feluda --yaml

Feluda prints the same structured data in YAML, matching the schema outlined in :ref:`reference`.

Run gist mode when you want a one-line summary for dashboards or comment bots.

.. code-block:: bash

   feluda --gist

Feluda condenses the report into a minimal single line.

----

Write reports to disk
---------------------

Saving results is useful before attaching them to tickets or :ref:`automate-integrate`.

Use this option when you need Feluda to write output straight to a file.

.. code-block:: bash

   feluda --output-file reports/feluda.txt

Feluda writes the requested format to the file and exits cleanly, making artifact uploads easy.
