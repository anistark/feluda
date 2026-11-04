:description: Feluda update and upgrade commands for keeping Feluda itself current.

.. _cli-update:

update and upgrade
==================

.. rst-class:: lead

   Keep Feluda itself current, through whatever installed it.

----

Overview
--------

Both commands work out how this copy of Feluda was installed, find the release to move to, and
hand over to the tool that owns the install.

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Command
     - Moves to
   * - ``feluda update``
     - The newest minor or patch release of the installed major version
   * - ``feluda upgrade``
     - The newest release, including a new major version

``feluda upgrade`` does not need a new major version to exist: when the newest release is a minor
or patch release, it installs that, as ``feluda update`` would.

----

Basic Usage
-----------

.. code-block:: bash

   # Stay on the installed major version
   feluda update

   # Take the newest release there is
   feluda upgrade

   # See what would be installed and how, without changing anything
   feluda update --check
   feluda upgrade --check

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--check``
     - Show the install method, the new version and the command, without changing anything

----

Install Methods
---------------

.. list-table::
   :header-rows: 1
   :widths: 22 33 45

   * - Installed with
     - Recognised by
     - Runs
   * - cargo
     - The binary is in ``~/.cargo/bin`` or ``$CARGO_HOME/bin``
     - ``cargo install feluda --version <version> --locked``
   * - Homebrew
     - The binary is in the Cellar
     - ``brew upgrade feluda``
   * - AUR
     - ``pacman`` owns the binary
     - ``paru -S feluda``, or ``yay -S feluda``
   * - ``.deb`` package
     - ``dpkg`` owns the binary
     - Downloads the package from the GitHub release, then ``sudo dpkg -i``
   * - ``.rpm`` package
     - ``rpm`` owns the binary
     - Downloads the package from the GitHub release, then ``sudo rpm -Uvh``
   * - pkgsrc
     - The binary is under ``/usr/pkg``
     - ``sudo pkgin install feluda``

``sudo`` is left out when you are already root. Output from these tools goes straight to your
terminal, so a password prompt or a confirmation works as usual, and a ``cargo install`` that
compiles for several minutes is not cut short.

A copy Feluda cannot place, such as one built from source and moved to ``/usr/local/bin``, is left
as it is. The command says so and exits with status 1.

On Windows, a running program cannot be replaced, so with a cargo install Feluda prints the command
for you to run.

----

Major Versions
--------------

cargo, ``.deb`` and ``.rpm`` installs take an exact version, so ``feluda update`` can install the
newest ``1.x`` even after ``2.0.0`` is out. It finishes by telling you the new major version exists.

Homebrew, the AUR and pkgsrc only install the newest version they offer. Once a new major version
is out, ``feluda update`` leaves those installs alone rather than cross it, and points you to
``feluda upgrade``.

----

Package Managers That Lag
-------------------------

Homebrew, the AUR and pkgsrc can trail a release by hours or days. After installing, Feluda reads
the version back from ``feluda --version`` and reports what it finds, so when the package manager
had nothing newer to offer, you are told ``feluda`` is still on the old version rather than that the
update worked. The same message appears when another copy of Feluda comes first on ``PATH``.

----

Where Versions Come From
------------------------

Most installs ask the crates.io index, skipping yanked versions.

``.deb`` and ``.rpm`` installs ask the GitHub releases API instead, since that is where the package
files are, skipping drafts and prereleases. They also skip any release that has no package for your
architecture yet, which is every release until its build finishes. That API allows 60
unauthenticated requests an hour per address, which a shared office or CI address can run out of:
``--github-token`` or ``GITHUB_TOKEN`` raises the limit, as it does for a scan.

To read from a mirror, set ``FELUDA_UPDATE_INDEX`` or ``FELUDA_UPDATE_RELEASES``. Your GitHub token
is not sent to a mirror.

.. note::

   These two can only be set in the environment. ``.feluda.toml`` is read from the current directory,
   and a cloned project should not be able to choose what gets installed with ``sudo``.
