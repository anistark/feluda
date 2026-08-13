:description: Feluda CLI command reference and usage guide.

.. _cli:

Feluda CLI
==========

.. rst-class:: lead

   Put Feluda on the case with deliberate commands, filters, and output modes tailored to each investigation.

----

Feluda provides a comprehensive command-line interface for scanning dependencies, generating compliance artifacts, and managing license detection. Each command is designed to fit seamlessly into both interactive workflows and automated CI/CD pipelines.

Every command carries its own reference. The fastest way through this page is to keep
one terminal open and ask Feluda directly.

.. raw:: html

   <div class="term-illustration" role="figure" aria-label="A terminal running feluda --help">
     <div class="term-chrome" aria-hidden="true">
       <span class="term-dot"></span>
       <span class="term-dot"></span>
       <span class="term-dot"></span>
       <span class="term-chrome-title">feluda</span>
     </div>
     <div class="term-body">
       <code><span class="term-prompt" aria-hidden="true">$</span>feluda --help<span class="term-cursor" aria-hidden="true"></span></code>
     </div>
   </div>

Append ``--help`` to any subcommand for its own flags, such as ``feluda sbom --help``.

Command Overview
----------------

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Command
     - Description
   * - ``feluda``
     - Scan dependencies and detect licenses
   * - ``feluda --filesystem``
     - Catalogue the OS packages installed under a root filesystem
   * - ``feluda watch``
     - Continuously re-scan when dependency files change
   * - ``feluda cache``
     - View and manage the license cache
   * - ``feluda generate``
     - Create NOTICE and THIRD_PARTY_LICENSES files
   * - ``feluda sbom``
     - Generate and validate Software Bill of Materials
