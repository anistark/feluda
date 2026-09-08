:description: Scan a container image for license compliance with Feluda.

.. _cli-containers:

Scan a Container Image
======================

.. rst-class:: lead

   Three routes from an image to a license verdict, and why none of them is ``--image``.

----

Overview
--------

Feluda has no flag that takes an image reference. What it has is two scan sources that between
them cover the case completely: :ref:`sbom-ingest` reads an inventory another tool produced, and
:ref:`cli-filesystem` catalogues a tree directly. An image becomes one or the other first.

Which route to take depends on what you already run, not on what you are scanning.

.. list-table::
   :header-rows: 1
   :widths: 25 35 40

   * - Route
     - Use when
     - What it costs
   * - Pipe syft
     - syft is already in the pipeline
     - Another tool to install, and its cataloguing rather than Feluda's
   * - Export the image
     - Docker is available locally
     - Disk for the extracted tree
   * - Copy with skopeo
     - CI has no Docker daemon
     - Another tool to install, but no daemon and no credentials in Feluda

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

Export and Scan the Tree
------------------------

``docker export`` flattens a container to a tarball, which Feluda scans with no other tool in the
pipeline:

.. code-block:: bash

   docker create --name tmp nginx:latest
   docker export tmp | tar -x -C rootfs
   docker rm tmp
   feluda --filesystem rootfs --fail-on-restrictive

This reads apk, dpkg and rpm databases plus installed Python and Node artifacts, and for the OS
packages it needs no network at all, since their licenses are already in the tree. It feeds the
document writers too:

.. code-block:: bash

   feluda sbom spdx --filesystem rootfs --output nginx.spdx.json

The resulting document describes what the image ships rather than what a source tree declares.

----

Without a Docker Daemon
-----------------------

CI runners often have no daemon. ``skopeo`` pulls straight from a registry into an OCI layout, and
handles the credentials itself:

.. code-block:: bash

   skopeo copy docker://nginx:latest dir:./rootfs-layers
   # then extract the layers in manifest order and scan the result
   feluda --filesystem rootfs

``crane export`` does the same job in one step if you prefer it. Either way Feluda never sees a
registry credential.

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
and the three routes above already cover the case. Feluda would rather own license resolution well
than own credential handling at all.

What is worth building is the half below that line: reading a ``docker save`` tarball or an OCI
layout directly, which is layer squashing and whiteout handling over catalogers that already exist,
with no network and no credentials. That is proposed in `issue #265
<https://github.com/anistark/feluda/issues/265>`_ and would collapse the export step above into
one command. A registry client stays unfiled until someone asks for it by name, with a workflow
where materialising the image locally is genuinely not an option.

----

Known Gaps
----------

For images these routes do not fully cover, catalogue with syft and ingest the result:

.. list-table::
   :header-rows: 1
   :widths: 45 55

   * - Gap
     - Tracking
   * - rpm ndb and Berkeley DB backends, so SUSE, openSUSE and CentOS 7 era images
     - `#263 <https://github.com/anistark/feluda/issues/263>`_
   * - Go build info, so distroless Go images report nothing
     - `#264 <https://github.com/anistark/feluda/issues/264>`_
   * - Installed Ruby gemspecs and jar manifests
     - `#254 <https://github.com/anistark/feluda/issues/254>`_
   * - ``docker save`` tarballs and OCI layouts as a direct source
     - `#265 <https://github.com/anistark/feluda/issues/265>`_
