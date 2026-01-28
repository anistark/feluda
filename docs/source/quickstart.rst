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

1. Install the binary (``cargo install feluda`` is the fastest route) so the detective is available on your ``PATH``.
2. Export a GitHub token if you expect large scans or private repositories.

   .. important::
      ``export GITHUB_TOKEN=<token>`` unlocks 5,000 GitHub API requests/hour and keeps Feluda from stalling mid-investigation.

3. Run ``feluda`` inside your project directory, or add ``--path``/``--language`` options if you want Feluda to focus on one workspace.
4. Capture structured evidence with ``feluda --json`` or ``feluda --yaml`` whenever teammates or CI jobs need machine-readable reports.

Ready for deeper filters, cache tips, and reporting workflows? Head to :ref:`cli` and :ref:`sbom` once installation is complete.
