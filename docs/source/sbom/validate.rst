:description: Validate SBOM files with Feluda.

.. _sbom-validate:

validate
========

.. rst-class:: lead

   Validate SBOM files against their format specifications to catch schema errors before customers or regulators do.

----

Basic Validation
----------------

Run a quick conformance check against an existing SBOM file.

.. code-block:: bash

   feluda sbom validate sbom.spdx.json

Feluda reads the file, validates it against the format, and reports pass/fail.

----

Save Validation Report
----------------------

Capture the validation summary when auditors request proof.

**SPDX validation:**

.. code-block:: bash

   feluda sbom validate sbom.spdx.json --output sbom-spdx-validation.txt

Feluda writes the outcome to ``sbom-spdx-validation.txt`` for archiving.

**CycloneDX validation:**

.. code-block:: bash

   feluda sbom validate sbom.cyclonedx.json --output sbom-cyclonedx-validation.txt

Feluda records CycloneDX validation notes separately.

----

JSON Output
-----------

Switch on JSON output when machines—not humans—read the validation log.

.. code-block:: bash

   feluda sbom validate spdx.json --json

Feluda prints a JSON report with line numbers and schema references.

**Combine with output flag:**

.. code-block:: bash

   feluda sbom validate spdx.json --json --output validation-report.json

Feluda stores the structured report so downstream systems can parse it later.

----

Options
-------

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--output <PATH>``
     - Save validation report to file
   * - ``--json``
     - Output validation report as JSON

.. important::
   Validation respects the ``--json`` flag only for the report itself; the exit code still reflects success or failure for CI consumption.

----

What Gets Validated
-------------------

Feluda validates:

- **Schema conformance** - Required fields, correct data types
- **SPDX identifiers** - Valid license identifiers
- **Format version** - SPDX 2.3 or CycloneDX v1.5 compliance
- **Document structure** - Proper nesting and relationships

----

CI/CD Integration
-----------------

Validate SBOMs as part of your release pipeline:

.. code-block:: bash

   # Generate SBOMs
   feluda sbom spdx --output sbom.spdx.json
   feluda sbom cyclonedx --output sbom.cyclonedx.json

   # Validate both formats
   feluda sbom validate sbom.spdx.json --output sbom-spdx-validation.txt
   feluda sbom validate sbom.cyclonedx.json --output sbom-cyclonedx-validation.txt

   # Machine-readable validation
   feluda sbom validate sbom.spdx.json --json --output validation.json

Validation failure returns a non-zero exit code, halting the pipeline if SBOMs don't conform.
