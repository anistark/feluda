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
restriction flags, and OSI status. Each entry also carries the ``ecosystem`` it
was resolved from and the ``purl`` (:ref:`package URL <output-purl>`) built from
it. When scanning a workspace or monorepo, entries additionally carry a
``sub_project`` field listing the workspace member(s) that pull in that
dependency. That field is omitted on single-project scans.

.. code-block:: text

   [
     {
       "name": "serde",
       "version": "1.0.219",
       "license": "MIT",
       "is_restrictive": false,
       "compatibility": "Compatible",
       "osi_status": "approved",
       "ecosystem": "cargo",
       "purl": "pkg:cargo/serde@1.0.219"
     }
   ]

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

.. _output-purl:

Package URLs
^^^^^^^^^^^^

A name and version identify a package only within one ecosystem: an npm
``libssl3`` and a Debian package of the same name are different software. Every
entry therefore carries a `package URL <https://github.com/package-url/purl-spec>`_
of the form ``pkg:<type>/<namespace>/<name>@<version>``, which is unique across
ecosystems and is what SBOM consumers key on.

.. list-table::
   :header-rows: 1
   :widths: 20 20 60

   * - Language
     - ``ecosystem``
     - Example ``purl``
   * - Rust
     - ``cargo``
     - ``pkg:cargo/serde@1.0.219``
   * - Node.js
     - ``npm``
     - ``pkg:npm/%40babel/core@7.24.0``
   * - Go
     - ``golang``
     - ``pkg:golang/github.com/pkg/errors@v0.9.1``
   * - Python
     - ``pypi``
     - ``pkg:pypi/flask-sqlalchemy@3.1.1``
   * - Java
     - ``maven``
     - ``pkg:maven/com.fasterxml.jackson.core/jackson-databind@2.17.0``
   * - Ruby
     - ``gem``
     - ``pkg:gem/rails@7.1.3``
   * - .NET
     - ``nuget``
     - ``pkg:nuget/Newtonsoft.Json@13.0.3``
   * - R
     - ``cran``
     - ``pkg:cran/ggplot2@3.5.1``
   * - C / C++
     - ``conan``, ``generic``
     - ``pkg:conan/fmt@10.2.1``, ``pkg:generic/libz@system``

Names are normalized the way each ecosystem defines: npm and Go names are
lowercased, Python names follow PEP 503, an npm scope and a Maven group become
the PURL namespace. Findings from the source and vendored scans have no registry
behind them and carry ``generic`` coordinates built from their path.

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
