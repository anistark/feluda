:description: Scan a container image for license compliance with Feluda.

.. _cli-containers:

Scan a Container Image
======================

.. rst-class:: lead

   Four routes from an image to a license verdict, and why none of them is ``--image``.

----

Overview
--------

Feluda has no flag that takes an image reference. What it has is three scan sources that between
them cover the case completely: :ref:`cli-image-archive` reads an image the way ``docker save``
or an OCI layout writes it out, :ref:`cli-filesystem` catalogues an extracted tree directly, and
:ref:`sbom-ingest` reads an inventory another tool produced. An image becomes one of those first.

Which route to take depends on what you already run, not on what you are scanning.

.. list-table::
   :header-rows: 1
   :widths: 25 35 40

   * - Route
     - Use when
     - What it costs
   * - Save the image
     - Docker, podman or nerdctl is available locally
     - One tarball on disk, nothing else in the pipeline
   * - Copy with skopeo
     - CI has no Docker daemon
     - Another tool to install, but no daemon and no credentials in Feluda
   * - Export the image
     - You already have the tree, or want to look at it
     - Disk for the extracted tree
   * - Pipe syft
     - syft is already in the pipeline
     - Another tool to install, and its cataloguing rather than Feluda's

----

Save and Scan the Archive
-------------------------

``docker save`` writes an image out as one tarball. Feluda reads it directly, squashing the layers
and honouring the whiteouts itself, with no other tool in the pipeline:

.. code-block:: bash

   docker save nginx:latest > nginx.tar
   feluda --image-archive nginx.tar --fail-on-restrictive

The same flag takes an OCI image layout directory or an OCI archive, which is what ``podman save
--format oci-dir``, ``skopeo copy ... oci:``, ``docker buildx build --output type=oci`` and
``crane pull`` produce, and any of the tar forms gzipped or zstd compressed. A multi platform
archive is not guessed at: ``--platform linux/arm64`` says which image to take, and leaving it out
lists what there is. See :ref:`cli-image-archive`.

What comes out is exactly what :ref:`cli-filesystem` reports for the extracted tree, since the
squashed filesystem goes through the same catalogers. It feeds the document writers too:

.. code-block:: bash

   feluda sbom spdx --image-archive nginx.tar --output nginx.spdx.json

----

Without a Docker Daemon
-----------------------

CI runners often have no daemon. ``skopeo`` pulls straight from a registry into an OCI layout, and
handles the credentials itself:

.. code-block:: bash

   skopeo copy docker://nginx:latest oci:./nginx:latest
   feluda --image-archive ./nginx --fail-on-restrictive

``crane pull nginx:latest nginx.tar`` does the same job in one step if you prefer it; the tarball
it writes is a ``docker save`` archive. Either way Feluda never sees a registry credential.

----

Export and Scan the Tree
------------------------

``docker export`` flattens a container to a tarball, which is the tree ``--image-archive`` builds
for itself, already on disk:

.. code-block:: bash

   docker create --name tmp nginx:latest
   docker export tmp | tar -x -C rootfs
   docker rm tmp
   feluda --filesystem rootfs --fail-on-restrictive

This is the route when the tree is already there, or when you want to look at it as well as scan
it. It reads apk, dpkg and rpm databases, installed Python and Node artifacts, and the build info
in every Go binary, so a distroless Go image reports the modules compiled into it. For the OS
packages it needs no network at all, since their licenses are already in the tree.

----

Pipe an Existing Cataloguer
---------------------------

syft, Trivy and cdxgen all catalogue images well. What they do not do is resolve, classify or gate:
they report whichever license string the package metadata carried and stop. That is where Feluda
starts.

.. code-block:: bash

   syft nginx:latest -o spdx-json | feluda --sbom-input - --fail-on-restrictive

``-`` reads from stdin, so nothing touches disk. The same path takes a vendor's SBOM, which is
often the only inventory you get for an image you did not build. See :ref:`sbom-ingest`.

----

Why There Is No ``--image``
---------------------------

Pulling an image by reference means Feluda would speak the OCI distribution API itself, and almost
none of that work is about licenses:

- **Reference parsing.** ``nginx:latest`` means ``docker.io/library/nginx:latest``, and
  ``localhost:5000/app:v1`` has a colon that is a port and a colon that is a tag.
- **Authentication.** The bearer token exchange, then ``~/.docker/config.json``, then credential
  helpers invoked as subprocesses, then ECR, Artifact Registry and ACR each doing it their own way,
  then rate limit handling for anonymous pulls.
- **Manifests.** A tag usually resolves to an index, so a platform has to be chosen, and buildx
  attestation manifests in that index have to be skipped rather than scanned as layers.
- **Blobs.** Digest verification, decompression, CDN redirects where the auth header must be
  dropped, retries and caching.

Registry authentication is the largest ongoing support surface in every scanner in this category,
and the four routes above already cover the case. Feluda would rather own license resolution well
than own credential handling at all.

The half below that line is built: ``--image-archive`` reads a ``docker save`` tarball or an OCI
layout directly, which is layer squashing and whiteout handling over catalogers that already exist,
with no network and no credentials. A registry client stays unfiled until someone asks for it by
name, with a workflow where materialising the image locally is genuinely not an option.

----

Known Gaps
----------

For images these routes do not fully cover, catalogue with syft and ingest the result:

.. list-table::
   :header-rows: 1
   :widths: 45 55

   * - Gap
     - Where it shows up
   * - The rpm Berkeley DB backend
     - CentOS 7, RHEL 8 and Amazon Linux 2 era images, which keep ``Packages`` rather than
       ``rpmdb.sqlite`` or ``Packages.db``. Feluda reports the backend it found and stops, rather
       than reporting nothing and reading as a clean scan.
   * - Installed Ruby gemspecs and jar manifests
     - Images that ship a Rails application or a JVM service. The OS packages and every other
       ecosystem in the tree are still catalogued.

.. code-block:: bash

   syft centos:7 -o spdx-json | feluda --sbom-input - --fail-on-restrictive

Neither gap has an issue open against it yet. If one of them is in your way, please open one and
say which image it bit you on.
