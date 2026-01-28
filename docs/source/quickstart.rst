:description: Quickstart into Feluda

.. _quickstart:

Quickstart
==========

.. rst-class:: lead

   Learn how to install quickly get **Feluda** on your system and start running.

----

Quick start briefing
--------------------

Feluda likes to set the scene before he inspects a repo, so walk through this short checklist after picking an installer below.

1. Install the binary so the detective is available on your ``PATH``.

.. code-block:: shell

   # Using Cargo (Rust's package manager)
   cargo install feluda

   # Using Homebrew (macOS/Linux)
   brew install feluda

2. Export a GitHub token if you expect large scans or private repositories.

   .. important::
      ``export GITHUB_TOKEN=<token>`` unlocks 5,000 GitHub API requests/hour and keeps Feluda from stalling mid-investigation.

3. Run ``feluda`` inside your project directory, or add ``--path``/``--language`` options if you want Feluda to focus on one workspace.

.. code-block:: shell

   feluda

Ready for deeper filters, cache tips, and reporting workflows? Head to :ref:`cli` and :ref:`sbom` once installation is complete.

----

Badge Adoption
--------------

Every solved case deserves credit. Showcase Feluda coverage in your README:

.. code-block:: markdown

   [![Scanned with Feluda](https://img.shields.io/badge/Scanned%20with-Feluda-brightgreen)](https://github.com/anistark/feluda)

The badge links back to Feluda and can be automatically updated via the GitHub Action's ``update-badge`` option.
