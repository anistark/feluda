:description: Start your investigation with Feluda, from installation to a first scan.
.. _getting-started:

Getting Started
===============

.. rst-class:: lead

   Set up Feluda like a calm detective preparing the case file before the chase begins.

----

Quick Start in 5 Steps
----------------------

Feluda favors orderly investigation, so follow these five deliberate moves before diving deeper in :ref:`using-feluda`.

#. Install Feluda with Cargo to get the freshest build straight from crates.io.

   Run this command when you want the officially published binary immediately.

   .. code-block:: bash

      cargo install feluda

   Feluda lands in your ``PATH`` so the sleuth can be summoned from any repo.

#. Prepare a GitHub token if you expect heavy scans or private repos.

   Use this export when you want Feluda to stay within the comfortable 5,000 requests/hour limit.

   .. code-block:: bash

      export GITHUB_TOKEN=<your_token>

   Feluda reads the variable quietly, and :ref:`using-feluda` explains how the token changes cache usage.

#. Start your first scan right from the project root.

   Run this command when you need Feluda to inspect the current directory without filters.

   .. code-block:: bash

      feluda

   Feluda prints a table of dependencies, restrictive flags, and compatibility verdicts.

#. Focus on one ecosystem if your repo hosts multiple languages.

   Trigger this scan when you only want, for example, the Node.js workspace examined.

   .. code-block:: bash

      feluda --language node

   Feluda narrows the report to the requested ``{rust|node|go|python|c|cpp|r}`` scope.

#. Capture structured output so compliance peers can reuse the findings.

   Use this option when you plan to archive or ship the evidence to another tool.

   .. code-block:: bash

      feluda --json

   Feluda emits JSON that mirrors the columns shown in the default view.

----

Install Feluda Anywhere
-----------------------

Feluda prides himself on meeting you where you work; tab through the paths, pop open dropdowns, and note the maintainer callouts for each route.

.. tab-set::
   :class: outline

   .. tab-item:: Cargo (Official)

      .. tip::
         Cargo installs are signed and published alongside Feluda releases.

      Run this when you want the direct Rust install path with zero distro lag.

      .. code-block:: bash

         cargo install feluda

      Feluda updates immediately whenever you rerun the Cargo install command.

   .. tab-item:: Linux Packages

      .. dropdown:: **Debian & Ubuntu (.deb)**
         :animate: fade-in

         Use this when apt-friendly systems prefer managed packages.

         .. code-block:: bash

            sudo dpkg -i feluda_*.deb
            sudo apt install -f

         Feluda registers with dpkg, so upgrades and removals follow normal workflows.

      .. dropdown:: **Fedora, RHEL, CentOS (.rpm)**
         :animate: fade-in

         Choose this route when rpm-based environments need a native handoff.

         .. code-block:: bash

            sudo dnf install feluda_*.rpm

         Fall back to the classic rpm flow when dnf is unavailable.

         .. code-block:: bash

            sudo rpm -ivh feluda_*.rpm

         Feluda installs through ``rpm`` with progress output, matching legacy documentation.

         Use the yum variant for environments that still rely on yum repositories.

         .. code-block:: bash

            sudo yum install feluda_*.rpm

         Feluda lets yum resolve dependencies first and then finishes the installation.

   .. tab-item:: Community Taps

      .. note::
         These packages are lovingly tracked by community maintainers; say thanks when you can.

      .. dropdown:: **Homebrew on macOS**
         :animate: fade-in

         .. admonition:: Maintainer
            :class: tip

            Brew formula maintained by `@chenrui333 <https://github.com/chenrui333>`_.

         Run this when macOS pipelines rely on brew installations.

         .. code-block:: bash

            brew install feluda

         Homebrew installs Feluda under ``/usr/local`` or ``/opt/homebrew`` depending on your architecture.

         Use this upgrade command whenever you want the newest release.

         .. code-block:: bash

            brew upgrade feluda

         Feluda updates in place, matching the rest of your brew-managed tools.

      .. dropdown:: **Arch Linux (AUR)**
         :animate: fade-in

         .. admonition:: Maintainer
            :class: tip

            AUR recipe maintained by `@adamperkowski <https://github.com/adamperkowski>`_.

         Use this command whenever you prefer an AUR helper.

         .. code-block:: bash

            paru -S feluda

         The helper builds the package locally, so Feluda aligns with Arch rolling releases.

      .. dropdown:: **NetBSD pkgsrc**
         :animate: fade-in

         .. admonition:: Maintainer
            :class: tip

            pkgsrc entry cared for by `@0323pin <https://github.com/0323pin>`_.

         Run this when NetBSD hosts or pkgsrc users seek Feluda through the official catalog.

         .. code-block:: bash

            pkgin install feluda

         pkgsrc places Feluda under ``/usr/pkg/bin`` so CI can pick it up instantly.

   .. tab-item:: Build from Source

      .. important::
         Source builds may expose experimental detective gadgets not yet shipped in releases.

      Clone the repo when you need to test a branch or custom patch.

      .. code-block:: bash

         git clone https://github.com/anistark/feluda.git

      Cargo builds the optimized binary once the sources arrive locally.

      .. code-block:: bash

         cd feluda && cargo build --release

      Copy the binary into a directory on your ``PATH`` to keep Feluda callable across shells.

      .. code-block:: bash

         sudo mv target/release/feluda /usr/local/bin/

      Feluda now runs the exact commit you checked out, handy for validating upcoming features described in :ref:`reference`.

----

Pick Your Distribution Channel
------------------------------

Feluda’s partners log every delivery so you can pick the right courier.

.. list-table::
   :header-rows: 1
   :widths: 20 20 60

   * - Channel
     - Maintainer
     - When to choose it
   * - Cargo crate
     - Core Feluda maintainers
     - You want the official release with predictable SemVer updates.
   * - DEB / RPM archives
     - Feluda release engineers
     - You need pinned artifacts for fleets under apt, dnf, yum, or rpm.
   * - Homebrew tap
     - @chenrui333
     - macOS hosts should stay in step with Apple silicon and Intel formulas.
   * - Arch AUR
     - @adamperkowski
     - Rolling release fans prefer rebuilding locally via paru or yay.
   * - NetBSD pkgsrc
     - @0323pin
     - pkgsrc clusters benefit from quarterly rollups.
   * - Source build
     - You + upstream
     - Patches, forks, or pre-release testing demand custom binaries.

----

Next detective moves
--------------------

- Visit :ref:`using-feluda` to master filters, caching, and GUI output.
- Jump to :ref:`compliance-reporting` once legal partners request NOTICE, SBOM, or badges.
- Bookmark :ref:`reference` for environment variables, config schema, and troubleshooting.
