:description: Integrate Feluda with GitHub Actions for automated license compliance.

.. _github-actions:

GitHub Actions
==============

.. rst-class:: lead

   Automate license compliance checks on every push and pull request with the official Feluda GitHub Action.

----

Quick Start
-----------

Add this workflow to ``.github/workflows/feluda.yml``:

.. code-block:: yaml

   name: Feluda Scan
   on:
     push:
       branches: [ main ]
     pull_request:
       branches: [ main ]
   jobs:
     scan:
       runs-on: ubuntu-latest
       steps:
         - uses: actions/checkout@v4
         - uses: anistark/feluda@v1
           with:
             fail-on-restrictive: true
             fail-on-incompatible: true
             update-badge: true

Feluda now exits non-zero whenever restrictive or incompatible licenses show up, and ``update-badge`` refreshes the README badge.

----

Action Inputs
-------------

.. list-table::
   :header-rows: 1
   :widths: 30 15 55

   * - Input
     - Default
     - Description
   * - ``fail-on-restrictive``
     - ``false``
     - Fail the workflow when restrictive licenses are found
   * - ``fail-on-incompatible``
     - ``false``
     - Fail the workflow when incompatible licenses are found
   * - ``update-badge``
     - ``false``
     - Update the Feluda badge in README
   * - ``language``
     - (all)
     - Filter scan to a specific language ecosystem
   * - ``project-license``
     - (none)
     - Declare project license for compatibility checks
   * - ``strict``
     - ``false``
     - Treat unknown licenses as incompatible

----

Complete Workflow Example
-------------------------

Full workflow with compliance artifacts and SBOM generation:

.. code-block:: yaml

   name: Feluda Compliance
   on:
     push:
       branches: [ main ]
     pull_request:
       branches: [ main ]

   jobs:
     scan:
       runs-on: ubuntu-latest
       steps:
         - uses: actions/checkout@v4

         - name: Run Feluda Scan
           uses: anistark/feluda@v1
           with:
             fail-on-restrictive: true
             fail-on-incompatible: true
             update-badge: true

         - name: Generate Compliance Artifacts
           run: |
             echo "1" | feluda generate
             echo "2" | feluda generate
             feluda sbom spdx --output sbom.spdx.json
             feluda sbom cyclonedx --output sbom.cyclonedx.json
             feluda sbom validate sbom.spdx.json --output sbom-spdx-validation.txt
             feluda sbom validate sbom.cyclonedx.json --output sbom-cyclonedx-validation.txt

         - name: Upload Compliance Artifacts
           uses: actions/upload-artifact@v4
           with:
             name: compliance-artifacts
             path: |
               NOTICE
               THIRD_PARTY_LICENSES.md
               sbom.spdx.json
               sbom.cyclonedx.json
               sbom-spdx-validation.txt
               sbom-cyclonedx-validation.txt

----

GitHub Token Configuration
--------------------------

Large scans may hit GitHub API rate limits. Store a ``GITHUB_TOKEN`` secret with ``repo`` scope for private dependencies.

.. code-block:: yaml

   - uses: anistark/feluda@v1
     env:
       GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
     with:
       fail-on-restrictive: true

.. tip::
   The default ``GITHUB_TOKEN`` provided by GitHub Actions works for public repositories. Use a Personal Access Token for private dependency scanning.

----

Scan Remote Repository
----------------------

Scan an external repository from your workflow:

.. code-block:: yaml

   - name: Scan External Repo
     run: |
       feluda --repo git@github.com:org/private-repo.git \
         --ssh-key "$HOME/.ssh/ci_key" \
         --ssh-passphrase "${{ secrets.SSH_PASSPHRASE }}"

.. important::
   Supply either SSH or HTTPS credentials—not both—unless your CI job truly needs the fallback.

----

Badge Updates
-------------

Keep the README badge current by enabling ``update-badge``:

.. code-block:: yaml

   - uses: anistark/feluda@v1
     with:
       update-badge: true

The action updates the badge snippet so your README always reflects the most recent scan status.

**Badge markdown:**

.. code-block:: markdown

   [![Scanned with Feluda](https://img.shields.io/badge/Scanned%20with-Feluda-brightgreen)](https://github.com/anistark/feluda)

----

Workflow Triggers
-----------------

Common trigger configurations:

**On every push and PR:**

.. code-block:: yaml

   on:
     push:
       branches: [ main, develop ]
     pull_request:
       branches: [ main ]

**Scheduled nightly scan:**

.. code-block:: yaml

   on:
     schedule:
       - cron: '0 2 * * *'  # 2 AM UTC daily

**Manual dispatch:**

.. code-block:: yaml

   on:
     workflow_dispatch:
       inputs:
         language:
           description: 'Language to scan'
           required: false
           default: ''
