:description: Feluda output formats and display options.

.. _cli-output:

output
======

.. rst-class:: lead

   Control how Feluda presents scan results with various output formats and display modes.

----

Output Formats
--------------

Different consumers prefer different shapes of the same evidence.

JSON Format
^^^^^^^^^^^

Machine-readable JSON for downstream automation.

.. code-block:: bash

   feluda --json

Feluda emits a JSON array containing dependency names, versions, licenses,
restriction flags, and OSI status. When scanning a workspace or monorepo, each
entry also carries a ``sub_project`` field listing the workspace member(s) that
pull in that dependency. The field is omitted on single-project scans.

YAML Format
^^^^^^^^^^^

YAML integrates better with configuration management tools.

.. code-block:: bash

   feluda --yaml

Feluda prints the same structured data in YAML format.

Gist Mode
^^^^^^^^^

A one-line summary for dashboards or comment bots.

.. code-block:: bash

   feluda --gist

Feluda condenses the report into a minimal single line.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Flag
     - Description
   * - ``--json``
     - Output as JSON array
   * - ``--yaml``
     - Output as YAML
   * - ``--gist``
     - Single-line summary output

----

Display Modes
-------------

GUI Mode
^^^^^^^^

Browse dependencies in Feluda's terminal user interface.

.. code-block:: bash

   feluda --gui

Feluda launches the graphical interface, letting you scroll through dependencies with OSI and compatibility badges.

Verbose Mode
^^^^^^^^^^^^

Extra columns including OSI status in standard output.

.. code-block:: bash

   feluda --verbose

Feluda adds OSI status and extended descriptions to the CLI table. In a
workspace or monorepo scan, the verbose table also includes a **Sub-project**
column showing which workspace member(s) own each dependency.

Debug Mode
^^^^^^^^^^

Detailed logging to troubleshoot license lookups.

.. code-block:: bash

   feluda --debug

Feluda outputs step-by-step details about file discovery, API calls, and cache hits.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Flag
     - Description
   * - ``--gui``
     - Launch terminal user interface
   * - ``--verbose``
     - Show extended information
   * - ``--debug``
     - Enable debug logging

----

Write Reports to Disk
---------------------

Save results before attaching them to tickets or CI artifacts.

.. code-block:: bash

   feluda --output-file reports/feluda.txt

Feluda writes the requested format to the file and exits cleanly, making artifact uploads easy.

**Combine with format flags:**

.. code-block:: bash

   # Save JSON report
   feluda --json --output-file reports/feluda.json

   # Save YAML report
   feluda --yaml --output-file reports/feluda.yaml

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Flag
     - Description
   * - ``--output-file <PATH>``
     - Write output to the specified file

----

CI Format
---------

Format output for CI consoles with platform-specific annotations.

**GitHub Actions:**

.. code-block:: bash

   feluda --ci-format github

Feluda writes ``::error`` and ``::warning`` annotations that GitHub parses automatically.

**Jenkins:**

.. code-block:: bash

   feluda --ci-format jenkins

Feluda formats its output with Jenkins-style prefixes to improve log parsing and highlighting.

**SARIF (GitHub Advanced Security / VS Code):**

.. code-block:: bash

   feluda --ci-format sarif --output-file results.sarif

Feluda emits a `SARIF 2.1.0 <https://sarifweb.azurewebsites.net/>`_ document.
Upload it to GitHub Advanced Security to surface findings in the Security tab and
in VS Code's Problems panel. A clean scan still produces a valid SARIF file with an
empty ``results`` array, so CI workflows can unconditionally upload the artifact.

.. code-block:: yaml

   - run: feluda --ci-format sarif --output-file results.sarif
   - uses: github/codeql-action/upload-sarif@v3
     with:
       sarif_file: results.sarif

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Value
     - Description
   * - ``github``
     - GitHub Actions annotation format
   * - ``jenkins``
     - Jenkins-compatible log markers (JUnit XML)
   * - ``sarif``
     - SARIF 2.1.0 for GitHub Advanced Security and VS Code
