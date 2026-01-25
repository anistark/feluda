:description: Generate compliance files, SBOMs, and badges with Feluda.

.. _compliance-reporting:

Compliance & Reporting
======================

.. rst-class:: lead

   Give legal, security, and partner teams the paperwork they expect without losing Feluda’s detective flair.

----

Generate NOTICE and THIRD_PARTY_LICENSES
----------------------------------------

Feluda guides you through interactive generation and scripted exports alike.

Run the interactive mode when you want Feluda to ask which artifact to create.

.. code-block:: bash

   feluda generate

Feluda prompts for NOTICE or THIRD_PARTY_LICENSES and renders the chosen file in-place.

Invoke this when you need to target a specific ecosystem and outbound license.

.. code-block:: bash

   feluda generate --language rust --project-license MIT

Feluda limits its analysis to the requested language and projects compatibility calculations against MIT.

Point Feluda elsewhere when compliance files must live in another workspace.

.. code-block:: bash

   feluda generate --path /opt/service

Feluda writes the selected artifacts into ``/opt/service`` with the relevant dependencies included.

.. note::
   Use ``NOTICE`` for concise attribution summaries and ``THIRD_PARTY_LICENSES`` when downstream consumers require the full license texts.

----

Produce SBOMs
-------------

Security teams expect an SBOM at every release, and Feluda can emit both SPDX and CycloneDX formats.

Run this when you need both formats at once for maximum compatibility.

.. code-block:: bash

   feluda sbom

Feluda creates ``SPDX`` and ``CycloneDX`` JSON files with metadata, license data, and timestamps.

Call the SPDX subcommand when you want just that specification.

.. code-block:: bash

   feluda sbom spdx

Feluda prints the SPDX JSON to stdout, ready for redirection or immediate uploads.

Use the output flag when you must persist the SPDX SBOM to disk.

.. code-block:: bash

   feluda sbom spdx --output sbom.spdx.json

Feluda saves the SPDX document to ``sbom.spdx.json`` and logs the path.

Run the CycloneDX variation when customers or tooling require it.

.. code-block:: bash

   feluda sbom cyclonedx

Feluda creates a CycloneDX v1.5 JSON structure with components, licenses, and hashes as available.

Capture the CycloneDX output to a file when you want reproducible releases.

.. code-block:: bash

   feluda sbom cyclonedx --output sbom.cyclonedx.json

Feluda writes the CycloneDX document alongside your build artifacts.

Use the combined command when you prefer both formats stored under one directory.

.. code-block:: bash

   feluda sbom --output sbom-output

Feluda drops files like ``sbom-output/spdx.json`` and ``sbom-output/cyclonedx.json`` so CI can upload them together.

----

Validate SBOM output
--------------------

Validation catches schema errors before customers or regulators do.

Run this when you want a quick conformance check against an existing SBOM file.

.. code-block:: bash

   feluda sbom validate sbom.spdx.json

Feluda reads the file, validates it against the format, and reports pass/fail.

Capture the validation summary when auditors request proof.

.. code-block:: bash

   feluda sbom validate sbom.spdx.json --output sbom-spdx-validation.txt

Feluda writes the outcome to ``sbom-spdx-validation.txt`` for archiving.

Repeat the process for CycloneDX files when you need parity.

.. code-block:: bash

   feluda sbom validate sbom.cyclonedx.json --output sbom-cyclonedx-validation.txt

Feluda records CycloneDX validation notes separately.

Switch on JSON output when machines—not humans—read the validation log.

.. code-block:: bash

   feluda sbom validate spdx.json --json

Feluda prints a JSON report with line numbers and schema references.

Use both JSON and output flags when you want machine-readable reports saved to disk.

.. code-block:: bash

   feluda sbom validate spdx.json --json --output validation-report.json

Feluda stores the structured report so downstream systems can parse it later.

.. important::
   Validation respects the ``--json`` flag only for the report itself; the exit code still reflects success or failure for CI consumption.

----

Pick the right SBOM
-------------------

.. list-table::
   :header-rows: 1
   :widths: 20 40 40

   * - Format
     - Use when
     - Contains
   * - SPDX 2.3
     - Sharing with open-source offices, regulators, or vulnerability scanners.
     - Dependency list, licenses, SPDX identifiers, and Feluda metadata.
   * - CycloneDX v1.5
     - Integrating with SBOM-first security tooling or commercial marketplaces.
     - Components, hashes, dependency graph hints, and license notes.

----

Adopt the Feluda badge
----------------------

Every solved case deserves credit.

Use this Markdown snippet when you want to showcase Feluda coverage in README or docs.

.. code-block:: bash

   printf '[![Scanned with Feluda](https://img.shields.io/badge/Scanned%%20with-Feluda-brightgreen)](https://github.com/anistark/feluda)\n' >> BADGE.md

The badge links back to Feluda, and :ref:`automate-integrate` explains how ``update-badge`` in the GitHub Action keeps it current.

----

Next compliance steps
---------------------

- Pair ``feluda generate`` with ``feluda sbom`` in CI following :ref:`automate-integrate`.
- Reference :ref:`configuration` when tailoring restrictive or ignore lists for repeated runs.
- Share badge and SBOM artifacts with partners alongside the NOTICE files for a complete package.
