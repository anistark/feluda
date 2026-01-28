:description: Generate and validate Software Bill of Materials with Feluda.

.. _sbom:

SBOM
====

.. rst-class:: lead

   Give legal, security, and partner teams the Software Bill of Materials they expect.

----

Overview
--------

Security teams expect an SBOM at every release, and Feluda can emit both SPDX and CycloneDX formats. SBOMs provide a comprehensive inventory of software components, their licenses, and dependencies.

Generate Both Formats
---------------------

Create both SPDX and CycloneDX files at once for maximum compatibility.

.. code-block:: bash

   feluda sbom

Feluda creates ``SPDX`` and ``CycloneDX`` JSON files with metadata, license data, and timestamps.

**Save to a directory:**

.. code-block:: bash

   feluda sbom --output sbom-output

Feluda drops files like ``sbom-output/spdx.json`` and ``sbom-output/cyclonedx.json`` so CI can upload them together.

----

Choosing the Right Format
-------------------------

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

Compliance Artifacts
--------------------

Pair SBOM generation with other compliance files:

.. code-block:: bash

   # Generate NOTICE and THIRD_PARTY_LICENSES
   echo "1" | feluda generate
   echo "2" | feluda generate

   # Generate SBOMs
   feluda sbom spdx --output sbom.spdx.json
   feluda sbom cyclonedx --output sbom.cyclonedx.json

   # Validate SBOMs
   feluda sbom validate sbom.spdx.json
   feluda sbom validate sbom.cyclonedx.json
