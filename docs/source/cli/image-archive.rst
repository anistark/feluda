:description: Scan a docker save tarball or an OCI image layout for license compliance with Feluda.

.. _cli-image-archive:

Scan an Image Archive
=====================

.. rst-class:: lead

   Read a container image the way ``docker save`` writes it out, with no registry in the loop.

----

Overview
--------

:ref:`cli-filesystem` catalogues an extracted tree, which meant an image had to be exported first.
``--image-archive`` takes the image as the tools write it out and does the export itself:

.. code-block:: bash

   docker save app:latest > app.tar
   feluda --image-archive app.tar --fail-on-restrictive

The layers are squashed in manifest order into a temporary root filesystem, whiteouts and all, and
that tree goes through the same catalogers ``--filesystem`` uses: apk, dpkg and rpm databases,
installed Python and Node artifacts, and the build info in every Go binary. What is reported is
exactly what ``docker export`` followed by ``--filesystem`` reports, from one command and one file.

There is no registry client, no credential handling and no network in any of this. For why that
line is drawn where it is, see :ref:`cli-containers`.

----

What Is Accepted
----------------

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Input
     - Written by
   * - ``docker save`` tarball
     - ``docker save``, ``podman save``, ``nerdctl save``, ``crane pull``
   * - OCI image layout directory
     - ``skopeo copy ... oci:dir``, ``podman save --format oci-dir``, ``crane pull --format=oci``,
       ``docker buildx build --output type=oci,dest=dir``
   * - OCI archive (the layout tarred up)
     - ``skopeo copy ... oci-archive:file.tar``, ``docker buildx build --output type=oci``
   * - Any of the tar forms, gzip or zstd compressed
     - ``docker save app | gzip > app.tar.gz``

The format is told from the archive's own contents. ``index.json`` at the top marks an OCI layout,
which Docker 25 and later write alongside the legacy ``manifest.json``; only the legacy file marks
an older ``docker save``. Layer compression is sniffed from each layer's first bytes rather than
trusted from its media type, since the two disagree often enough to matter.

A tarball is indexed once by walking its headers and then read by seeking, so a layer is streamed
rather than held in memory. A compressed tarball is decompressed to a temporary file first, because
``docker save`` writes the manifest last and a compressed stream cannot seek to it.

----

Choosing a Platform
-------------------

An archive can hold more than one image: a multi platform build, or several images saved together.
Feluda does not guess which one you meant.

.. code-block:: console

   $ feluda --image-archive app.tar
   ❌ Image archive error: The archive holds 2 images; choose one with --platform. Available: app:latest (linux/amd64), app:latest (linux/arm64/v8)

   $ feluda --image-archive app.tar --platform linux/arm64

``--platform`` takes ``os/arch`` or ``os/arch/variant``, as ``docker --platform`` does. A request
without a variant matches any variant, so ``linux/arm64`` finds a ``linux/arm64/v8`` image. An
archive holding one image needs no ``--platform``; if one is given it has to match.

buildx writes provenance and SBOM attestations into a multi platform index as manifests for the
platform ``unknown/unknown``. They hold no filesystem and are never offered as a choice.

----

How Layers Are Squashed
-----------------------

Each layer is applied on top of the last, into a temporary directory that is removed when the scan
finishes. An image's worth of disk is the price of reusing the filesystem catalogers unchanged; it
is also exactly what ``docker export | tar -x`` costs.

Whiteouts follow the `OCI layer specification
<https://github.com/opencontainers/image-spec/blob/main/layer.md#whiteouts>`_. A ``.wh.<name>``
entry removes ``<name>`` from the layers below it, and ``.wh..wh..opq`` removes everything below
under its directory. Both hide only what lower layers put there: a file the same layer writes stays,
whichever order the two appear in the tar. So a package installed in one layer and removed in the
next is not reported, which is the acceptance test the feature was built against.

Two things are deliberately not carried over from the layers. File modes are not applied, since a
directory a lower layer made read-only would block the next layer from writing into it and nothing
downstream reads permissions. And every write is checked to land inside the temporary root once
symlinks resolve, with anything already at the destination removed first, so a layer that ships a
symlink to ``/etc`` cannot make a later layer's ``etc/passwd`` a write on the host.

----

SBOM Generation
---------------

The same source feeds the document writers, on ``sbom`` and on each format:

.. code-block:: bash

   feluda sbom spdx --image-archive app.tar --output app.spdx.json
   feluda sbom cyclonedx --image-archive app.tar --platform linux/amd64 --output app.cdx.json

The document describes what the image ships rather than what its source declares, with the distro
in each OS package's PURL (``pkg:apk/alpine/musl@1.2.5-r0``) exactly as :ref:`cli-filesystem`
writes it.

----

Combining Flags
---------------

``--image-archive`` replaces the manifest scan and cannot be combined with ``--repo``,
``--sbom-input`` or ``--filesystem``. ``--platform`` requires it. ``--path`` stays available and
supplies the project license that compatibility is checked against. ``feluda watch`` re-scans
dependency files and does not accept it.

Every output mode, filter and CI gate applies unchanged:

.. code-block:: bash

   feluda --image-archive app.tar --json
   feluda --image-archive app.tar --restrictive
   feluda --image-archive app.tar --project-license MIT --fail-on-incompatible
   feluda --image-archive app.tar --ci-format github

----

Not Covered
-----------

Pulling by reference (``--image nginx:latest``) is not on the roadmap; :ref:`cli-containers` says
why, and shows ``skopeo`` and ``crane`` producing an archive without a daemon. Whatever
:ref:`cli-filesystem` does not catalogue yet, this source does not either, since it is the same
catalogers underneath.
