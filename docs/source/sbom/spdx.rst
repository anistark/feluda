:description: Generate SPDX 2.3, 2.2 or 3.0 format SBOMs with Feluda, as JSON or tag:value.

.. _sbom-spdx:

SPDX
====

.. rst-class:: lead

   Generate Software Package Data Exchange (SPDX) format SBOMs for compliance and security reporting.

----

Overview
--------

SPDX is an open standard for communicating software bill of material information, including components, licenses, copyrights, and security references. Feluda generates SPDX 2.3 compliant documents by default, and SPDX 2.2 or 3.0 on request. SPDX
2.x is written as JSON or tag:value.

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
   * - ``--spec-version <VERSION>``
     - SPDX version to write: ``2.3`` (default), ``2.2`` or ``3.0``. See :ref:`sbom-versions`
   * - ``--format <FORMAT>``
     - ``json`` (default) or ``tag-value``. Tag:value is SPDX 2.x only

----

Tag:value
---------

Some consumers still take SPDX's line based format. Ask for it with ``--format tag-value``:

.. code-block:: bash

   feluda sbom spdx --format tag-value --output sbom.spdx

The file gets ``.spdx`` appended unless its name already ends that way. It states exactly what the
JSON document would: the same packages, SPDX ids, PURL references and licenses, with every
``LicenseRef-feluda-*`` defined in a ``LicenseID`` block at the end. Both 2.2 and 2.3 pass the
reference ``pyspdxtools`` validator.

----

SPDX 3.0
--------

.. code-block:: bash

   feluda sbom spdx --spec-version 3.0 --output sbom.spdx.json

SPDX 3.0 is a new model rather than a revision. The document is JSON-LD: an ``@context`` and a
flat ``@graph`` of elements addressed by ``spdxId``. Feluda writes 3.0.1, which is what that
context and the official schema describe.

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - SPDX 2.x
     - SPDX 3.0
   * - ``packages[]``
     - ``software_Package`` elements, with ``software_packageVersion`` and ``software_packageUrl``
   * - ``licenseDeclared``, ``licenseConcluded``
     - ``Relationship`` elements, ``hasDeclaredLicense`` and ``hasConcludedLicense``, pointing at a
       ``simplelicensing_LicenseExpression``. One expression element serves every package that
       states the same license
   * - ``hasExtractedLicensingInfos``
     - ``simplelicensing_SimpleLicensingText``, which an expression maps its ``LicenseRef-`` ids to
       through ``simplelicensing_customIdToUri``
   * - ``NOASSERTION``
     - No relationship
   * - ``DESCRIBES`` relationships
     - The ``software_Sbom`` element's ``rootElement``
   * - (no 2.x field)
     - ``software_sbomType``: ``source`` when the SBOM comes from a project's manifests,
       ``analyzed`` when it comes from ``--filesystem`` or ``--image-archive``
   * - ``creationInfo``
     - One shared ``CreationInfo``, created by a ``SoftwareAgent`` named Feluda using a ``Tool``

The output passes the official ``spdx3-validate`` (JSON schema and SHACL).

----

SPDX Document Contents
----------------------

The generated SPDX document includes:

- **Document metadata** - Creator info, creation timestamp, SPDX version
- **Package information** - Name, version, download location
- **Package coordinates** - A ``PACKAGE-MANAGER`` external reference carrying the package's PURL (``PACKAGE_MANAGER`` in 2.2)
- **License data** - SPDX license identifiers for each package
- **Relationships** - Dependency relationships between packages
- **Feluda metadata** - Tool version and scan parameters

Each package's ``SPDXID`` is derived from its PURL, so two packages that share a
name and version across ecosystems stay distinct elements in the document.

SPDX license fields only accept ids from the SPDX license list, expressions over them, and
``LicenseRef-`` ids the document defines. A license outside the list, such as ``SEE LICENSE IN
LICENSE.txt`` or a registry title like ``The Apache Software License, Version 2.0``, is written as
a ``LicenseRef-feluda-*`` id, for example ``LicenseRef-feluda-SEE-LICENSE-IN-LICENSE.txt``, and defined once in
``hasExtractedLicensingInfos`` with the text the package stated. Inside an expression only the
unlisted license becomes a reference, so ``Custom-1.0 OR MIT`` stays a choice:
``LicenseRef-feluda-Custom-1.0 OR MIT``.

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
       "creators": ["Tool: feluda-1.15.0"]
     },
     "packages": [
       {
         "name": "github.com/gin-gonic/gin",
         "SPDXID": "SPDXRef-Package-pkg090896e46f91ab5a",
         "versionInfo": "v1.12.0",
         "licenseDeclared": "MIT",
         "externalRefs": [
           {
             "referenceCategory": "PACKAGE-MANAGER",
             "referenceType": "purl",
             "referenceLocator": "pkg:golang/github.com/gin-gonic/gin@v1.12.0"
           }
         ]
       }
     ]
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
