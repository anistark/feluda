:description: Feluda CLI command reference and usage guide.

.. _cli:

Feluda CLI
==========

.. rst-class:: lead

   Put Feluda on the case with deliberate commands, filters, and output modes tailored to each investigation.

----

Feluda provides a comprehensive command-line interface for scanning dependencies, generating compliance artifacts, and managing license detection. Each command is designed to fit seamlessly into both interactive workflows and automated CI/CD pipelines.

Command Overview
----------------

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Command
     - Description
   * - ``feluda``
     - Scan dependencies and detect licenses
   * - ``feluda cache``
     - View and manage the license cache
   * - ``feluda generate``
     - Create NOTICE and THIRD_PARTY_LICENSES files
   * - ``feluda sbom``
     - Generate and validate Software Bill of Materials
