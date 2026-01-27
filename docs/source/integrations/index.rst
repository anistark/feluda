:description: Integrate Feluda with CI/CD pipelines and automation tools.

.. _integrations:

Integrations
============

.. rst-class:: lead

   Let Feluda sweep every pull request, nightly build, and remote repo so the detective never sleeps on compliance.

----

Overview
--------

Feluda integrates seamlessly with CI/CD platforms to automate license compliance checks, SBOM generation, and badge updates. Set it once and let every commit trigger a thorough dependency audit.

----

Supported Platforms
-------------------

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Platform
     - Integration Method
   * - GitHub Actions
     - Official Feluda Action (``anistark/feluda@v1``)
   * - Jenkins
     - Shell commands with ``--ci-format jenkins``
   * - GitLab CI
     - Shell commands with standard output
   * - Other CI/CD
     - Direct CLI invocation

----

Quick Start
-----------

**GitHub Actions (recommended):**

.. code-block:: yaml

   - uses: anistark/feluda@v1
     with:
       fail-on-restrictive: true
       fail-on-incompatible: true

**Jenkins:**

.. code-block:: bash

   feluda --ci-format jenkins --fail-on-restrictive --fail-on-incompatible

**Generic CI:**

.. code-block:: bash

   feluda --fail-on-restrictive --fail-on-incompatible

----

CI Output Formats
-----------------

Feluda adjusts its output to match the CI platform's annotation system.

.. code-block:: bash

   # GitHub Actions annotations
   feluda --ci-format github

   # Jenkins log markers
   feluda --ci-format jenkins

----

Full Compliance Workflow
------------------------

Automate the complete compliance artifact generation:

.. code-block:: bash

   # Run scan with CI formatting
   feluda --ci-format github --fail-on-restrictive --fail-on-incompatible

   # Generate attribution files
   echo "1" | feluda generate  # NOTICE
   echo "2" | feluda generate  # THIRD_PARTY_LICENSES

   # Generate SBOMs
   feluda sbom spdx --output sbom.spdx.json
   feluda sbom cyclonedx --output sbom.cyclonedx.json

   # Validate SBOMs
   feluda sbom validate sbom.spdx.json --output sbom-spdx-validation.txt
   feluda sbom validate sbom.cyclonedx.json --output sbom-cyclonedx-validation.txt
