:description: Generate and validate Software Bill of Materials with Feluda.

.. _sbom:

SBOM
====

.. rst-class:: lead

   Give legal, security, and partner teams the Software Bill of Materials they expect.

----

Overview
--------

Security teams expect an SBOM at every release, and Feluda can emit both SPDX and CycloneDX formats. SBOMs provide a comprehensive inventory of software components, their licenses, and dependencies.

Feluda also reads them. An SPDX or CycloneDX document from syft, Trivy, cdxgen or a vendor can be
analysed directly, which is how you cover a shipped container image rather than a source tree.
See :ref:`sbom-ingest`.

A filesystem or an image can be catalogued directly too, with no other tool involved:
``feluda sbom spdx --filesystem ./rootfs`` describes what an artifact ships rather than what its
source declares, and ``feluda sbom spdx --image-archive app.tar`` does the same for a ``docker
save`` tarball or an OCI layout. See :ref:`cli-filesystem` and :ref:`cli-image-archive`.

For container images specifically, :ref:`cli-containers` covers the routes from an image reference
to any of those sources.

Generate Both Formats
---------------------

Create both SPDX and CycloneDX files at once for maximum compatibility.

.. code-block:: bash

   feluda sbom

Feluda creates ``SPDX`` and ``CycloneDX`` JSON files with metadata, license data, and timestamps.

**Save to a directory:**

.. code-block:: bash

   feluda sbom --output sbom-output

Feluda drops files like ``sbom-output/spdx.json`` and ``sbom-output/cyclonedx.json`` so CI can upload them together.

----

Choosing the Right Format
-------------------------

.. list-table::
   :header-rows: 1
   :widths: 20 40 40

   * - Format
     - Use when
     - Contains
   * - SPDX 2.3 (or 2.2)
     - Sharing with open-source offices, regulators, or vulnerability scanners.
     - Dependency list, licenses, SPDX identifiers, and Feluda metadata.
   * - CycloneDX 1.6 (or 1.4, 1.5, 1.7)
     - Integrating with SBOM-first security tooling or commercial marketplaces.
     - Components, hashes, dependency graph hints, and license notes.

----

.. _sbom-versions:

Spec Versions
-------------

Feluda writes SPDX 2.3 and CycloneDX 1.6 by default. 1.6 is what syft, Trivy and cdxgen write, so
a Feluda BOM goes wherever theirs already do. When a consumer only accepts an older version, ask
for it:

.. code-block:: bash

   feluda sbom spdx --spec-version 2.2
   feluda sbom cyclonedx --spec-version 1.4

   # Both formats at once
   feluda sbom --spdx-version 2.2 --cyclonedx-version 1.5

To pin versions for a project, set them in ``.feluda.toml``; a flag still wins over the file:

.. code-block:: toml

   [sbom]
   spdx = "2.2"
   cyclonedx = "1.4"

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Version
     - What changes in the output
   * - SPDX 2.3
     - The default.
   * - SPDX 2.2
     - The PURL reference category is spelled ``PACKAGE_MANAGER``. ``licenseConcluded``,
       ``licenseDeclared`` and ``copyrightText`` are always present, as 2.2 requires.
   * - CycloneDX 1.7
     - Same content as 1.6.
   * - CycloneDX 1.6
     - The default. Each license is marked ``"acknowledgement": "declared"``: the license the
       package states in its manifest, registry entry or license file.
   * - CycloneDX 1.5
     - No ``acknowledgement``.
   * - CycloneDX 1.4
     - No ``acknowledgement``, and ``metadata.tools`` is the older plain list rather than
       ``{"components": [...]}``.

Reading is not tied to these versions: ``--sbom-input`` reads SPDX 2.2 and 2.3 and CycloneDX 1.2
to 1.7 JSON. See :ref:`sbom-ingest`.

----

Compliance Artifacts
--------------------

Pair SBOM generation with other compliance files:

.. code-block:: bash

   # Generate NOTICE and THIRD_PARTY_LICENSES
   echo "1" | feluda generate
   echo "2" | feluda generate

   # Generate SBOMs
   feluda sbom spdx --output sbom.spdx.json
   feluda sbom cyclonedx --output sbom.cyclonedx.json

   # Validate SBOMs
   feluda sbom validate sbom.spdx.json
   feluda sbom validate sbom.cyclonedx.json
