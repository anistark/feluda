:description: Automate Feluda in CI pipelines and integrations.
.. _automate-integrate:

Automate & Integrate
====================

.. rst-class:: lead

   Let Feluda sweep every pull request, nightly build, and remote repo so the detective never sleeps on compliance.

----

Wire up GitHub Actions
----------------------

This workflow calls the official Feluda action, fails on risky licenses, and keeps the badge fresh.

Use this workflow block when you want GitHub Actions to run Feluda on every change.

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

Feluda now exits non-zero whenever restrictive or incompatible licenses show up, and ``update-badge`` refreshes the README badge described in :ref:`compliance-reporting`.

.. tip::
   Store a ``GITHUB_TOKEN`` secret with ``repo`` scope if private dependencies might trigger GitHub API rate limits.

----

Automate compliance artifacts
-----------------------------

Pair Feluda runs with artifact uploads so auditors always receive NOTICE, THIRD_PARTY_LICENSES, and SBOM files.

Use this step when you want CI to produce everything in one go.

.. code-block:: bash

   feluda --ci-format github --fail-on-restrictive --fail-on-incompatible
   echo "1" | feluda generate
   echo "2" | feluda generate
   feluda sbom spdx --output sbom.spdx.json
   feluda sbom cyclonedx --output sbom.cyclonedx.json
   feluda sbom validate sbom.spdx.json --output sbom-spdx-validation.txt
   feluda sbom validate sbom.cyclonedx.json --output sbom-cyclonedx-validation.txt

Feluda prints GitHub-friendly annotations, writes compliance files, generates SBOMs, validates them, and signals failure if risks remain.

.. note::
   Follow up with ``actions/upload-artifact`` or your preferred uploader to preserve the generated files for legal review.

----

Run Feluda in Jenkins
---------------------

Many enterprises rely on Jenkins, and Feluda plays nicely there too.

Execute this scripted step when you want Jenkins to interpret Feluda output cleanly.

.. code-block:: groovy

   stage('Feluda Scan') {
     steps {
       sh '''
         feluda --ci-format jenkins --fail-on-restrictive --fail-on-incompatible
         feluda sbom --output build/sboms
       '''
       archiveArtifacts artifacts: 'NOTICE,THIRD_PARTY_LICENSES.md,build/sboms/*', fingerprint: true
     }
   }

Feluda emits Jenkins-friendly markers, and the archived artifacts keep SBOMs and compliance files tied to the build record.

----

Format output for CI consoles
-----------------------------

Use CI formatting flags whenever you want Feluda to print annotations that match the current platform.

Run this for GitHub workflows or any runner that understands GitHub CLI markers.

.. code-block:: bash

   feluda --ci-format github

Feluda writes ``::error`` and ``::warning`` annotations that GitHub parses automatically.

Run the Jenkins variation when you need console-friendly log markers.

.. code-block:: bash

   feluda --ci-format jenkins

Feluda formats its output with Jenkins-style prefixes to improve log parsing and highlighting.

----

Scan remote repos in CI
-----------------------

Feluda can examine a different repository than the current checkout, which is useful for centralized auditing jobs.

Use this command when you want a scheduled workflow to scan an external Git repository.

.. code-block:: bash

   feluda --repo git@github.com:org/private-repo.git --ssh-key "$HOME/.ssh/ci_key" --ssh-passphrase "$SSH_PASSPHRASE" --token "$HTTPS_TOKEN"

Feluda clones the repository with the provided credentials, scans it, and cleans up the temporary clone afterward.

.. important::
   Supply either SSH or HTTPS credentials—not both—unless your CI job truly needs the fallback.

----

Keep badges current
-------------------

Feluda’s action can rewrite the README badge each time a scan completes.

Run this action configuration when you want to display the latest verdict automatically.

.. code-block:: yaml

   - uses: anistark/feluda@v1
     with:
       update-badge: true

Feluda updates the badge snippet defined in :ref:`compliance-reporting`, so your README always reflects the most recent scan.

----

Next integration moves
----------------------

- Reuse the ``feluda sbom`` commands from :ref:`compliance-reporting` in nightly jobs for a continuous SBOM stream.
- Refer teammates back to :ref:`using-feluda` for manual cache clears or GUI reviews when CI finds issues.
- Customize restrictive or ignore lists via :ref:`reference` before running Feluda in production pipelines.

