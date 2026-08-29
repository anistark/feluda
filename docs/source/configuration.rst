:description: Configure Feluda’s policies, ignores, and environment overrides.

.. _configuration:

Configuration
=============

.. rst-class:: lead

   Tune Feluda’s instincts—restrictive lists, dependency ignores, compatibility rules, and caches—before every deep investigation.

----

Understand the default posture
------------------------------

Feluda ships with a conservative restrictive list so risky licenses stand out on day one.

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Default category
     - Contents
   * - Restrictive licenses
     - ``GPL-1.0``, ``GPL-2.0``, ``GPL-3.0``, ``AGPL-3.0``, ``LGPL-3.0``, ``MPL-2.0``, ``SEE LICENSE IN LICENSE``, ``CC-BY-SA-4.0``, ``EPL-2.0``
   * - Cache location
     - ``.feluda/cache/github_licenses.json`` refreshed automatically every 30 days
   * - Compatibility data
     - ``config/license_compatibility.toml`` mapping project licenses to allowed dependency licenses

.. note::
   Feluda validates the defaults at runtime and warns if incompatible combinations slip into your overrides.

----

Customize .feluda.toml
----------------------

Feluda reads a `.feluda.toml` file in your project root to override the restrictive and ignore lists.

Use this template when you want to declare an updated restrictive posture.

.. code-block:: toml

   [licenses]
   restrictive = [
       "GPL-3.0",
       "AGPL-3.0",
       "Custom-1.0",
   ]

   ignore = [
       "MIT",
       "Apache-2.0",
   ]

Feluda merges these settings with its defaults so you can safely extend or narrow the watch list.

Run this command whenever you want to make sure the new configuration takes effect immediately.

.. code-block:: bash

   feluda --restrictive

Feluda now outputs only dependencies whose licenses appear in the merged restrictive list, confirming the override works.

.. important::
   Avoid listing the same license under both ``restrictive`` and ``ignore``—Feluda will warn and ignore the duplicate, but your policies will be unclear to future readers.

----

Ignore dependencies deliberately
--------------------------------

Some dependencies warrant exemptions, such as internal libraries or dev-only tooling.

Use this snippet when you need to ignore an entire dependency or a specific version.

.. code-block:: toml

   [[dependencies.ignore]]
   name = "github.com/anistark/wasmrun"
   version = "v1.0.0"
   reason = "Internal component that shares the main project license."

   [[dependencies.ignore]]
   name = "internal-library"
   version = ""
   reason = "Covered by a separate legal agreement across all versions."

Feluda removes matching dependencies from every report, keeping NOTICE files and SBOMs focused on external packages.

.. tip::
   Leave ``version`` empty to ignore every release; fill it out to scope the exemption to one build only.

----

Teach Feluda a license it does not know
---------------------------------------

Feluda's built-in rules recognise the common license texts, but some projects
ship one those rules cannot place: a bespoke internal license, a rewording of a
standard one, or a standard text wrapped in a custom preamble. Those land in the
report as Unknown. A custom definition names the phrases that identify such a
text and the id to report when they all appear.

Use this snippet when a license text keeps resolving to Unknown.

.. code-block:: toml

   [[licenses.custom]]
   id = "LicenseRef-acme-internal"
   match_all = ["ACME CONFIDENTIAL", "Internal Use Only"]
   restrictive = true

Feluda now reports ``LicenseRef-acme-internal`` for any license file containing
both phrases, and treats it as restrictive, so ``--fail-on-restrictive`` and
every filter apply to it like any other license.

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Field
     - Meaning
   * - ``id``
     - The identifier to report. Use the real SPDX id when the text is a known
       license Feluda misses, or a ``LicenseRef-`` prefixed id (the SPDX
       convention for licenses outside the register) for something bespoke.
   * - ``match_all``
     - Phrases that must **all** appear in the license text. Matching is
       case-sensitive on plain substrings, so lift phrases straight out of the
       file. At least one is required.
   * - ``restrictive``
     - Optional. Whether a match counts as restrictive. Leave it out to
       classify ``id`` through the license registry and the ``restrictive``
       list above.

Custom definitions are tried before the built-in rules, so they also correct a
text Feluda places wrongly. Repeat the same ``id`` across several entries to
accept more than one wording of one license, and list the most specific rules
first, because the first match wins.

.. tip::
   ``restrictive`` works in both directions. Set it to ``false`` to stop
   flagging a license your legal team has already cleared, which is narrower
   than adding the license to ``ignore``, since the dependency still appears in
   reports, NOTICE files, and SBOMs.

.. note::
   Definitions match on license **text**, so they apply wherever Feluda reads a
   license file: dependency license fallbacks, vendored directories, and stray
   ``LICENSE`` files. They do not rewrite an id a package manifest declares
   outright, and they cannot override a filename that implies an id on its own,
   such as ``OFL.txt``.

----

Manage compatibility rules
--------------------------

Feluda checks every dependency license against ``config/license_compatibility.toml``.

Refer to this structure when you need to add or override compatibility data.

.. code-block:: toml

   [MIT]
   compatible_with = [
       "MIT",
       "BSD-2-Clause",
       "BSD-3-Clause",
       "Apache-2.0",
       "ISC",
   ]

   [GPL-3.0]
   compatible_with = [
       "GPL-3.0",
       "LGPL-3.0",
       "AGPL-3.0",
       "MIT",
       "Apache-2.0",
   ]

After editing the file, rerun Feluda with your project license declared to validate the effect.

.. code-block:: bash

   feluda --project-license MIT

Feluda re-evaluates every dependency against the updated matrix and flags incompatibilities immediately.

.. note::
   Keep custom compatibility files under version control so legal reviewers can audit how the matrix evolved.

----

Control the ClearlyDefined fallback
-----------------------------------

Licenses Feluda cannot resolve on its own are looked up in ClearlyDefined. The lookup is on by
default and only ever runs for dependencies that already failed to resolve.

.. code-block:: toml

   [clearlydefined]
   enabled = true
   endpoint = "https://api.clearlydefined.io/definitions"

Set ``enabled = false`` when package names and versions must not leave the machine, or point
``endpoint`` at a mirror, a proxy, or a self-hosted instance. Both have environment equivalents:

.. code-block:: bash

   export FELUDA_CLEARLYDEFINED_ENABLED=false
   export FELUDA_CLEARLYDEFINED_ENDPOINT=https://clearlydefined.internal/definitions

``--no-clearlydefined`` turns it off for a single run. See :ref:`cli-clearlydefined`.

----

Control environment overrides
-----------------------------

Environment variables override both defaults and `.feluda.toml`, which is perfect for CI experiments.

Use this export when a pipeline needs a temporary restrictive list.

.. code-block:: bash

   export FELUDA_LICENSES_RESTRICTIVE='["GPL-3.0","AGPL-3.0","Custom-1.0"]'

Feluda reads the JSON string, merges it into the configuration, and respects it for the rest of the shell session.

Apply this override when you want to reduce noise from permissive licenses on a short-lived branch.

.. code-block:: bash

   export FELUDA_LICENSES_IGNORE='["MIT","Apache-2.0","BSD-3-Clause"]'

Feluda hides the listed licenses from reports until the environment variable is cleared.

Prepare tokens where rate limits might obstruct private scans.

.. code-block:: bash

   export GITHUB_TOKEN=<your_token>

Feluda automatically uses the token, raising the limit to 5,000 requests/hour and unlocking private repositories.

----

Validate configuration health
-----------------------------

Feluda validates configuration files every time it runs and surfaces warnings inline.

- Duplicate licenses: flagged so you can deduplicate before confusion spreads.
- Empty strings: treated as errors, prompting you to correct typos in `.feluda.toml`.
- Invalid SPDX identifiers: surfaced as warnings so you can confirm custom identifiers with legal teams.
- Duplicate dependency entries: rejected when both name and version collide.

.. important::
   Treat warnings as TODOs; Feluda will still run, but inaccurate config undermines compliance evidence.

----

Reset caches cleanly
--------------------

Cache files help Feluda avoid repeated GitHub calls, but you may need to purge them after changing tokens or policies.

Run this when you want to inspect cache size and expiration before clearing it.

.. code-block:: bash

   feluda cache

Feluda reports the cache path, entry count, and age so you can confirm whether a refresh is necessary.

Call this to wipe the cache after rotating credentials or troubleshooting mismatched data.

.. code-block:: bash

   feluda cache --clear

Feluda deletes ``.feluda/cache/github_licenses.json`` and rebuilds it on the next scan.
