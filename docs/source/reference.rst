:description: Reference tables, configuration schema, and troubleshooting notes for Feluda.
.. _reference:

Reference
=========

.. rst-class:: lead

   Keep this dossier handy when you need definitive answers about Feluda’s flags, config files, caches, and terminology.

----

CLI flag table
--------------

Use this table to double-check flag behavior before scripting.

.. list-table::
   :header-rows: 1
   :widths: 25 35 40

   * - Flag / Command
     - Purpose
     - Notes
   * - ``feluda --path <dir>``
     - Scan a different directory.
     - Accepts relative or absolute paths.
   * - ``feluda --repo <url>``
     - Clone and scan a remote repository.
     - Combine with ``--ssh-key``, ``--ssh-passphrase``, or ``--token`` for private access.
   * - ``feluda --language {rust|node|go|python|c|cpp|r}``
     - Limit analysis to one ecosystem.
     - Useful for monorepos or staged reviews.
   * - ``feluda --osi {approved|not-approved|unknown}``
     - Filter by OSI approval status.
     - Requires verbose, JSON, YAML, or GUI modes to display OSI columns clearly.
   * - ``feluda --restrictive`` / ``feluda --incompatible``
     - Show only restrictive or incompatible dependencies.
     - Relies on the restrictive list and compatibility matrix described below.
   * - ``feluda --project-license <SPDX>``
     - Evaluate compatibility against a declared license.
     - Supports MIT, Apache-2.0, GPL variants, MPL-2.0, BSD variants, ISC, 0BSD, Unlicense, WTFPL, and more.
   * - ``feluda --fail-on-restrictive`` / ``feluda --fail-on-incompatible``
     - Exit non-zero when risky findings exist.
     - Ideal for CI as in :ref:`automate-integrate`.
   * - ``feluda --no-local``
     - Skip local manifests and fetch data remotely.
     - Helpful when manifests are incomplete or stale.
   * - ``feluda --github-token <token>``
     - Pass a GitHub token inline.
     - Overridden by ``GITHUB_TOKEN`` env var when both are present.
   * - ``feluda cache`` / ``feluda cache --clear``
     - Inspect or delete the GitHub license cache.
     - Default cache path: ``.feluda/cache/github_licenses.json``.
   * - ``feluda --json`` / ``feluda --yaml`` / ``feluda --gist``
     - Switch output format.
     - JSON/YAML suit automation; gist prints a one-liner.
   * - ``feluda --verbose`` / ``feluda --gui``
     - Enrich the terminal display.
     - GUI launches a TUI; verbose adds OSI/compatibility columns.
   * - ``feluda --output-file <path>``
     - Save text output to a file.
     - Works with any format flag.
   * - ``feluda --ci-format {github|jenkins}``
     - Emit annotations suited to CI platforms.
     - Pairs with ``--fail-on-*`` for fully automated gates.

----

Configuration schema
--------------------

Customize Feluda via `.feluda.toml` to align with your company policy.

Use this template when defining restrictive and ignore lists.

.. code-block:: toml

   [licenses]
   restrictive = [
       "GPL-3.0",
       "AGPL-3.0",
       "LGPL-3.0",
       "MPL-2.0",
       "SEE LICENSE IN LICENSE",
       "CC-BY-SA-4.0",
       "EPL-2.0",
   ]
   ignore = [
       "MIT",
       "Apache-2.0",
       "BSD-2-Clause",
       "BSD-3-Clause",
       "ISC",
   ]

Feluda merges the defaults with your overrides and warns if a license appears in both lists.

Document dependency exceptions when certain packages deserve a pass.

.. code-block:: toml

   [[dependencies.ignore]]
   name = "github.com/anistark/wasmrun"
   version = "v1.0.0"
   reason = "Internal component that shares the project license."

   [[dependencies.ignore]]
   name = "internal-library"
   version = ""
   reason = "All versions are governed by a separate legal agreement."

Feluda removes matching dependencies entirely from scan results, keeping the audit trail tidy.

.. note::
   Leave ``version`` empty to ignore every release of a dependency; fill it to scope the ignore more narrowly.

----

Environment variables
---------------------

Environment overrides take precedence over config files.

Run this export when you want to redefine restrictive licenses for a single session.

.. code-block:: bash

   export FELUDA_LICENSES_RESTRICTIVE='["GPL-3.0","AGPL-3.0","Custom-1.0"]'

Feluda merges the JSON array into its restrictive list before scanning.

Use this when you need to ignore common permissive licenses temporarily.

.. code-block:: bash

   export FELUDA_LICENSES_IGNORE='["MIT","Apache-2.0","BSD-3-Clause"]'

Feluda hides the specified licenses from reports until the variable is unset.

Run this when you prefer to set the GitHub token once per shell instead of passing ``--github-token`` every time.

.. code-block:: bash

   export GITHUB_TOKEN=<your_token>

Feluda uses the supplied token to raise rate limits to 5,000 requests/hour.

----

Troubleshooting
---------------

.. important::
   Feluda stores fetched licenses in ``.feluda/cache/github_licenses.json``. Delete it via ``feluda cache --clear`` if you change tokens or suspect corruption.

- **GitHub rate limits:** Without authentication you receive only 60 requests/hour. Set ``GITHUB_TOKEN`` as shown above and confirm the cache displays the higher limit via ``feluda cache``.
- **Cache location:** When diagnosing mismatched data, double-check the timestamps printed by ``feluda cache`` to ensure the entries are fresh.
- **CI formatting:** If annotations fail to appear in GitHub or Jenkins, confirm the job uses ``feluda --ci-format`` with the correct platform specified.
- **Remote scans:** Ensure CI runners have access to SSH keys or HTTPS tokens before invoking ``feluda --repo`` to avoid authentication prompts.

----

Glossary
--------

- **Permissive license:** MIT, Apache-2.0, BSD, ISC, or similarly lenient licenses that allow broad redistribution.
- **Restrictive license:** GPL variants, AGPL, LGPL, MPL-2.0, EPL-2.0, CC-BY-SA-4.0, or any identifier listed under ``[licenses.restrictive]``.
- **Compatibility matrix:** The rules stored in ``config/license_compatibility.toml`` that determine whether dependency licenses mix safely with your project’s license.
- **NOTICE file:** Concise attribution summary generated via ``feluda generate``.
- **THIRD_PARTY_LICENSES file:** Full license-text compendium also produced by ``feluda generate``.
- **SBOM:** Software Bill of Materials containing dependency, license, and metadata exported via ``feluda sbom``.

----

Contributor resources
---------------------

- Review ``CONTRIBUTING.md`` in the repository root for code-style, testing, and submission expectations.
- Study ``ACTION-README.md`` for advanced GitHub Action usage, especially when combining ``update-badge`` with :ref:`automate-integrate`.
- Check ``config/license_compatibility.toml`` if you need to suggest compatibility changes; open a pull request with legal input.
- Explore ``examples/`` to see sample outputs that mirror ``feluda --json`` and ``feluda sbom`` results.

