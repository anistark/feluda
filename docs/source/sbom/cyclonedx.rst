:description: Generate CycloneDX v1.5 format SBOMs with Feluda.

.. _sbom-cyclonedx:

CycloneDX
=========

.. rst-class:: lead

   Generate CycloneDX format SBOMs for security tooling and commercial integrations.

----

Overview
--------

CycloneDX is a lightweight SBOM standard designed for use in application security contexts and supply chain component analysis. Feluda generates CycloneDX v1.5 compliant documents.

----

Generate CycloneDX SBOM
-----------------------

Create a CycloneDX document for your project.

.. code-block:: bash

   feluda sbom cyclonedx

Feluda creates a CycloneDX v1.5 JSON structure with components, licenses, and hashes as available.

----

Save to File
------------

Capture the CycloneDX output for reproducible releases.

.. code-block:: bash

   feluda sbom cyclonedx --output sbom.cyclonedx.json

Feluda writes the CycloneDX document alongside your build artifacts.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--output <PATH>``
     - Save CycloneDX document to the specified file

----

CycloneDX Document Contents
---------------------------

The generated CycloneDX document includes:

- **BOM metadata** - Serial number, version, timestamp, tool info
- **Components** - Package name, version, type, purl
- **Licenses** - License identifiers and expressions
- **Hashes** - SHA-256 and other integrity hashes when available
- **Dependencies** - Dependency graph and relationships

----

Example Output Structure
------------------------

.. code-block:: text

   {
     "bomFormat": "CycloneDX",
     "specVersion": "1.5",
     "serialNumber": "urn:uuid:...",
     "version": 1,
     "metadata": {
       "timestamp": "2025-01-27T12:00:00Z",
       "tools": [{"name": "feluda", "version": "1.11.1"}]
     },
     "components": []
   }

----

Use Cases
---------

CycloneDX format is ideal when:

- Integrating with SBOM-first security tooling (e.g., Dependency-Track)
- Submitting to commercial software marketplaces
- Working with DevSecOps pipelines that expect CycloneDX
- Meeting customer security questionnaire requirements
- Using vulnerability correlation tools

----

CI/CD Integration
-----------------

Generate and validate CycloneDX SBOMs in CI pipelines:

.. code-block:: bash

   feluda sbom cyclonedx --output sbom.cyclonedx.json
   feluda sbom validate sbom.cyclonedx.json --output sbom-cyclonedx-validation.txt

See :ref:`integrations` for complete CI/CD workflow examples.
