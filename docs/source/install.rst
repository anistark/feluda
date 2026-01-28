:description: How to install Feluda across different systems and package managers.

.. _install:

Installation
============

.. rst-class:: lead

   Learn how to install **Feluda** on your system using Cargo or your preferred package manager.

----

Overview
--------

Feluda is a Rust_-based command-line tool that analyzes a project’s dependencies, records their licenses,
and flags any permissions that restrict personal or commercial use or conflict with the project’s license.

----

cargo (recommended)
-------------------

The official Feluda crate is provided via the Rust Package Registry, Cargo_.

.. note::
   **Prerequisite:** Ensure the latest version of Rust is installed on your system.

.. tab-set::
   :class: outline

   .. tab-item:: :iconify:`devicon:rust` cargo

      .. code-block:: bash

         cargo install feluda

----

Community Maintained
--------------------

.. admonition:: Maintained by the community
   :class: tip

   Feluda is also available through several community package managers.
   Each method below includes installation steps and maintainers.

.. tab-set::
   :class: outline

   .. tab-item:: 🍺 Homebrew (macOS)

      Maintained by `@chenrui333 <https://github.com/chenrui333>`_

      Available on `Homebrew <https://formulae.brew.sh/formula/feluda>`_.

      .. code-block:: bash

         brew install feluda

   .. tab-item:: 🐧 Arch Linux (AUR)

      Maintained by `@adamperkowski <https://github.com/adamperkowski>`_

      Available in the `AUR <https://aur.archlinux.org/packages/feluda>`_.

      .. code-block:: bash

         paru -S feluda

   .. tab-item:: NetBSD

      Maintained by `@0323pin <https://github.com/0323pin>`_

      Available from the `official pkgsrc repositories <https://pkgsrc.se/devel/feluda/>`_.

      .. code-block:: bash

         pkgin install feluda

   .. tab-item:: 🐧 DEB (Debian/Ubuntu)

      Feluda is available as a DEB package for Debian-based systems.

      1. Download the latest ``.deb`` file from `GitHub Releases <https://github.com/anistark/feluda/releases>`_
      2. Install the package:

      .. code-block:: bash

         sudo dpkg -i feluda_*.deb
         # Fix any dependency issues
         sudo apt install -f

   .. tab-item:: 🎩 RPM (Fedora/RHEL)

      Feluda is available as an RPM package for Red Hat-based systems.

      1. Download the latest ``.rpm`` file from `GitHub Releases <https://github.com/anistark/feluda/releases>`_
      2. Install the package:

      .. code-block:: bash

         # Using rpm
         sudo rpm -ivh feluda_*.rpm

         # Using dnf (Fedora/newer RHEL)
         sudo dnf install feluda_*.rpm

         # Using yum (older RHEL/CentOS)
         sudo yum install feluda_*.rpm

----

Build from Source 🧪
--------------------

Building from source is recommended only for advanced users.
You’ll need to have Cargo_ and Git_ installed on your system.

.. dropdown:: **For Advanced Users**
   :animate: fade-in

   Note: This build may include experimental or unreleased features.

   .. code-block:: bash

      # Clone the repository
      git clone https://github.com/anistark/feluda.git
      cd feluda

      # Build the release binary
      cargo build --release

      # Move it to a directory in your PATH
      sudo mv target/release/feluda /usr/local/bin/

----

.. _Rust: https://www.rust-lang.org/
.. _Cargo: https://crates.io/
.. _Git: https://git-scm.com/
