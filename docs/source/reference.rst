:description: Reference tables, troubleshooting notes, and glossary entries for Feluda.

.. _reference:

Reference
=========

.. rst-class:: lead

   Keep this dossier handy when you need definitive answers about Feluda’s flags, troubleshooting steps, and terminology.

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
   * - ``feluda --language {rust|node|go|python|c|cpp|dotnet|r}``
     - Limit analysis to one ecosystem.
     - Useful for monorepos or staged reviews.
   * - ``feluda --osi {approved|not-approved|unknown}``
     - Filter by OSI approval status.
     - Requires verbose, JSON, YAML, or GUI modes to display OSI columns clearly.
   * - ``feluda --restrictive`` / ``feluda --incompatible``
     - Show only restrictive or incompatible dependencies.
     - Relies on the restrictive list and compatibility matrix described in :ref:`configuration`.
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
   * - ``feluda --debug`` / ``-d``
     - Enable debug mode with detailed logging.
     - Useful for troubleshooting detection issues.
   * - ``feluda --strict``
     - Enable strict mode for license parsing.
     - Treats unknown licenses as incompatible.
   * - ``feluda generate``
     - Generate NOTICE and THIRD_PARTY_LICENSES files.
     - Accepts ``--path``, ``--language``, ``--project-license``.
   * - ``feluda sbom [spdx|cyclonedx]``
     - Generate SBOM in SPDX 2.3 or CycloneDX v1.5 format.
     - Omit format to generate both; use ``--output`` to save.
   * - ``feluda sbom validate <file>``
     - Validate an SBOM file against its specification.
     - Supports ``--json`` for machine-readable output.

----

Need configuration guidance?
-----------------------------

Looking to adjust restrictive lists, dependency ignores, compatibility matrices, or environment overrides? Jump to :ref:`configuration` for detailed instructions, code snippets, and validation tips before you rerun Feluda.

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
