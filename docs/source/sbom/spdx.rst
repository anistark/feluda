:description: Generate SPDX 2.3 format SBOMs with Feluda.

.. _sbom-spdx:

SPDX
====

.. rst-class:: lead

   Generate Software Package Data Exchange (SPDX) format SBOMs for compliance and security reporting.

----

Overview
--------

SPDX is an open standard for communicating software bill of material information, including components, licenses, copyrights, and security references. Feluda generates SPDX 2.3 compliant documents.

----

Generate SPDX SBOM
------------------

Create an SPDX document for your project.

.. code-block:: bash

   feluda sbom spdx

Feluda prints the SPDX JSON to stdout, ready for redirection or immediate uploads.

----

Save to File
------------

Persist the SPDX SBOM to disk.

.. code-block:: bash

   feluda sbom spdx --output sbom.spdx.json

Feluda saves the SPDX document to ``sbom.spdx.json`` and logs the path.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--output <PATH>``
     - Save SPDX document to the specified file

----

SPDX Document Contents
----------------------

The generated SPDX document includes:

- **Document metadata** - Creator info, creation timestamp, SPDX version
- **Package information** - Name, version, download location
- **License data** - SPDX license identifiers for each package
- **Relationships** - Dependency relationships between packages
- **Feluda metadata** - Tool version and scan parameters

----

Example Output Structure
------------------------

.. code-block:: text

   {
     "spdxVersion": "SPDX-2.3",
     "dataLicense": "CC0-1.0",
     "SPDXID": "SPDXRef-DOCUMENT",
     "name": "project-sbom",
     "documentNamespace": "https://example.org/...",
     "creationInfo": {
       "created": "2025-01-27T12:00:00Z",
       "creators": ["Tool: feluda-1.11.1"]
     },
     "packages": []
   }

----

Use Cases
---------

SPDX format is ideal when:

- Sharing with open-source program offices (OSPO)
- Meeting regulatory compliance requirements
- Integrating with vulnerability scanners (e.g., Grype, Trivy)
- Submitting to government or enterprise procurement processes
- Participating in open-source foundations that require SPDX

----

CI/CD Integration
-----------------

Generate and upload SPDX SBOMs in CI pipelines:

.. code-block:: bash

   feluda sbom spdx --output sbom.spdx.json
   feluda sbom validate sbom.spdx.json --output sbom-spdx-validation.txt

See :ref:`integrations` for complete CI/CD workflow examples.
