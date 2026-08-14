:description: Catalogue the OS packages and language artifacts installed under a root filesystem with Feluda.

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
system's own package managers keep and the metadata that language installers leave next to the code
they installed.

.. code-block:: bash

   docker export app | tar -x -C rootfs
   feluda --filesystem rootfs --fail-on-restrictive

That makes a shipped container analysable with nothing else in the pipeline: no cataloguing tool
and no image handling. For the OS packages there is no network either, since their licenses are
already in the tree.

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
   * - pip, and anything that installs a wheel
     - Python distributions
     - ``*.dist-info/METADATA`` and ``*.egg-info/PKG-INFO``
   * - npm, pnpm, yarn
     - Node packages
     - The ``package.json`` inside each ``node_modules`` entry

The tree does not have to be a full root filesystem. An extracted image layer, a chroot, a mounted
disk, or an installation directory all work.

Every cataloger runs, so an image carrying more than one package manager's database is reported in
full, and so is the application installed on top of it. In most images the application's own
dependencies are the larger half: a base image contributes on the order of ninety dpkg packages,
and what is in ``site-packages`` or ``node_modules`` is what the image was built to run.

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

Installed language artifacts carry the PURL of their own ecosystem, exactly as they would from a
manifest scan, so a finding means the same thing wherever it came from:

.. code-block:: text

   pkg:pypi/requests@2.32.3
   pkg:npm/%40babel/core@7.24.0

----

One Library, One Finding
------------------------

A distribution's language packages install real artifacts. Debian's ``python3-yaml`` puts a PyYAML
distribution into ``dist-packages``, metadata directory and all, so without care the same library
would be reported twice, once as ``pkg:deb/debian/python3-yaml`` and once as ``pkg:pypi/pyyaml``.

Feluda suppresses the second by ownership rather than by name: dpkg records every file a package
installed in ``/var/lib/dpkg/info/<package>.list``, and apk records the same in its installed
database. An artifact whose metadata file appears in one of those lists is already in the report as
an OS package.

Nothing is matched on names, because ``python3-yaml`` to ``pyyaml`` is a guess that both over- and
under-suppresses. And a library installed in more than one place — a virtualenv beside the system
interpreter, a dependency hoisted into two ``node_modules`` trees — is one finding, since it is one
package at one version.

----

Licenses for Installed Artifacts
--------------------------------

Installed metadata states a license several ways, and Feluda reads them in descending order of
confidence.

For Python: ``License-Expression`` (PEP 639) first, since it is an SPDX expression by definition;
then a ``License`` field that states an expression; then the Trove ``License ::`` classifiers, which
come from a fixed vocabulary that maps onto SPDX; then the ``License`` field itself, matched as text
when a distribution has put its whole license in there.

Classifiers that name a family rather than a license — ``BSD License``, which covers three different
licenses, or ``GNU General Public License (GPL)``, which names no version — map to nothing. Picking
one would put a license in the distribution's mouth that it never claimed. Those fall through to the
license text the wheel shipped in its metadata directory, which does say which one it is.

For Node: the ``license`` field, including the legacy ``{"type": ...}`` object and ``licenses``
array; then the package's own ``LICENSE`` file, which its tarball ships. ``SEE LICENSE IN <file>``
names no license and is treated as unstated.

Anything still unresolved goes to the package's registry. This is the one thing a filesystem scan
can do for an installed artifact that it cannot do for an OS package: a distribution in
``site-packages`` is a real PyPI release, so there is somewhere to ask.

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

Pointing ``--filesystem`` at a tree with nothing installed in it at all is an error rather than an
empty report, so a mistyped path cannot read as a clean scan. A tree holding only installed
artifacts is fine: an installation directory like ``/opt/app`` has no package database behind it and
is still worth scanning.

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

RPM-based distributions, installed Ruby gemspecs, jars and Go build info are not catalogued yet.
Until they are, pipe syft's output through :ref:`sbom-ingest` for those cases:

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
