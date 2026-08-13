:description: Catalogue the OS packages installed under a root filesystem with Feluda.

.. _cli-filesystem:

Scan a Filesystem
=================

.. rst-class:: lead

   Report what an artifact actually ships, not what its source tree declares.

----

Overview
--------

Every other scan source answers "what did someone write down?": a manifest, or an SBOM another tool
produced. ``--filesystem`` answers "what is actually on this disk?", by reading the databases the
system's own package managers keep.

.. code-block:: bash

   docker export app | tar -x -C rootfs
   feluda --filesystem rootfs --fail-on-restrictive

That makes a shipped container analysable with nothing else in the pipeline: no cataloguing tool,
no registry client, no network. The licenses are already in the tree.

----

What Is Covered
---------------

.. list-table::
   :header-rows: 1
   :widths: 20 30 50

   * - Package manager
     - Distributions
     - Where the license comes from
   * - apk
     - Alpine
     - The ``L:`` field of ``/lib/apk/db/installed``, which records it directly
   * - dpkg
     - Debian, Ubuntu and derivatives
     - ``/usr/share/doc/<package>/copyright``, since dpkg's database has no license field at all

The tree does not have to be a full root filesystem. An extracted image layer, a chroot, a mounted
disk, or an installation directory all work, as long as one of those databases is in it.

Both catalogers run, so an image carrying more than one package manager's database is reported in
full.

----

Identity
--------

Packages carry the distribution in their PURL, taken from the tree's own ``/etc/os-release``:

.. code-block:: text

   pkg:deb/debian/libssl3@3.0.15-1
   pkg:apk/alpine/musl@1.2.5-r0

The distribution is part of a package's identity, not decoration: a Debian ``libssl3`` and an
Ubuntu one are different packages, and consumers matching Feluda's SBOM against another tool's need
to see which is which. A tree with no ``os-release`` file simply has no namespace, which is still a
valid PURL.

----

Debian License Names
--------------------

Debian's license short names predate SPDX and do not match it. Feluda translates them, so
classification and compatibility work the same way they do everywhere else:

.. list-table::
   :header-rows: 1
   :widths: 40 60

   * - Debian
     - SPDX
   * - ``GPL-2+``
     - ``GPL-2.0-or-later``
   * - ``LGPL-2.1+``
     - ``LGPL-2.1-or-later``
   * - ``Expat``, ``MIT/X11``
     - ``MIT``
   * - ``BSD-3-clause``
     - ``BSD-3-Clause``
   * - ``GPL-2+ or Artistic``
     - ``GPL-2.0-or-later OR Artistic-1.0``

Without this a GPL package would report as unknown rather than restrictive, and the gate the
feature exists for would stay green.

----

Unknown Licenses
----------------

A package whose license cannot be read is reported as unknown, never guessed at. Debian packages
that predate the machine-readable copyright format sometimes state their license only in prose that
points at ``/usr/share/common-licenses``, and Feluda will not infer a license from a reference. In a
stock ``debian:12-slim`` image this affects a handful of the 88 installed packages; the rest resolve.

Pointing ``--filesystem`` at a tree with no package database at all is an error rather than an empty
report, so a mistyped path cannot read as a clean scan.

----

SBOM Generation
---------------

The same source feeds the document writers:

.. code-block:: bash

   feluda sbom spdx --filesystem ./rootfs --output rootfs.spdx.json
   feluda sbom cyclonedx --filesystem ./rootfs --output rootfs.cdx.json

----

Not Yet Covered
---------------

RPM-based distributions and installed language artifacts (``site-packages``, ``node_modules``,
gemspecs, jars) are not catalogued yet. Until they are, pipe syft's output through
:ref:`sbom-ingest` for those cases:

.. code-block:: bash

   syft nginx:latest -o spdx-json | feluda --sbom-input - --fail-on-restrictive

----

Combining Flags
---------------

``--filesystem`` replaces the manifest scan and cannot be combined with ``--repo`` or
``--sbom-input``. ``--path`` stays available and supplies the project license that compatibility is
checked against. ``feluda watch`` re-scans dependency files and does not accept it.

Every output mode, filter and CI gate applies unchanged:

.. code-block:: bash

   feluda --filesystem ./rootfs --json
   feluda --filesystem ./rootfs --restrictive
   feluda --filesystem ./rootfs --project-license MIT --fail-on-incompatible
   feluda --filesystem ./rootfs --ci-format github
