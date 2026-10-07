:description: Generate CycloneDX 1.4 to 1.7 format SBOMs with Feluda.

.. _sbom-cyclonedx:

CycloneDX
=========

.. rst-class:: lead

   Generate CycloneDX format SBOMs for security tooling and commercial integrations.

----

Overview
--------

CycloneDX is a lightweight SBOM standard designed for use in application security contexts and supply chain component analysis. Feluda generates CycloneDX 1.6 compliant documents by default, and 1.4, 1.5 or 1.7 on request.

----

Generate CycloneDX SBOM
-----------------------

Create a CycloneDX document for your project.

.. code-block:: bash

   feluda sbom cyclonedx

Feluda creates a CycloneDX 1.6 JSON structure with components, licenses, and hashes as available.

Pick another version with ``--spec-version``:

.. code-block:: bash

   feluda sbom cyclonedx --spec-version 1.4

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
   * - ``--spec-version <VERSION>``
     - CycloneDX version to write: ``1.4``, ``1.5``, ``1.6`` (default) or ``1.7``. See :ref:`sbom-versions`

----

CycloneDX Document Contents
---------------------------

The generated CycloneDX document includes:

- **BOM metadata** - Serial number, version, timestamp, tool info
- **Components** - Package name, version, type, purl
- **Licenses** - License identifiers and expressions
- **Hashes** - SHA-256 and other integrity hashes when available
- **Dependencies** - Dependency graph and relationships

Every component carries a ``purl`` built from the ecosystem it was resolved
from, so components stay identifiable across ecosystems:

.. code-block:: text

   {
     "type": "library",
     "name": "@babel/core",
     "version": "7.24.0",
     "purl": "pkg:npm/%40babel/core@7.24.0",
     "licenses": [{"license": {"id": "MIT", "acknowledgement": "declared"}}]
   }

``acknowledgement`` is written from 1.6 on; 1.4 and 1.5 have no such field.

A license is written as ``id`` only when it is on the SPDX license list, spelled the list's way,
since that is all the CycloneDX schema accepts there. Anything else, such as ``SEE LICENSE IN
LICENSE.txt`` or a registry's own title like ``The Apache Software License, Version 2.0``, is
written as ``name``. Only text that contains quotes, backslashes, control characters or markup is
dropped to ``NOASSERTION``. ``feluda sbom validate`` warns
about an ``id`` that is not on the list.

----

Example Output Structure
------------------------

.. code-block:: text

   {
     "bomFormat": "CycloneDX",
     "specVersion": "1.6",
     "serialNumber": "urn:uuid:...",
     "version": 1,
     "metadata": {
       "timestamp": "2025-01-27T12:00:00Z",
       "tools": {
         "components": [{"type": "application", "name": "feluda", "version": "1.17.0"}]
       }
     },
     "components": []
   }

With ``--spec-version 1.4``, ``tools`` is the older plain list:
``[{"name": "feluda", "version": "1.17.0"}]``.

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
