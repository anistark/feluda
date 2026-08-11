:description: Analyse an existing SPDX or CycloneDX document with Feluda.

.. _sbom-ingest:

Scan an Existing SBOM
=====================

.. rst-class:: lead

   Run Feluda's policy over an inventory another tool produced.

----

Overview
--------

A source tree only describes what its manifests declare. The artifact you actually ship (a
container image, an appliance filesystem, a build a vendor handed you) contains far more than
that, and cataloguing it is what tools like syft, Trivy and cdxgen exist for.

What those tools do not do is *judge* what they find. They report whichever license string the
package metadata happened to carry, leave the rest as ``NOASSERTION``, and stop there. Feluda picks
up from that point: point ``--sbom-input`` at their output and every part of the normal pipeline
applies to it: license resolution, restrictive classification, compatibility against your project
license, and the CI gates.

.. code-block:: bash

   syft nginx:latest -o spdx-json | feluda --sbom-input - --fail-on-restrictive

``-`` reads the document from stdin, so the scanner never has to touch disk.

----

Reading a Document
------------------

Pass a file path, or ``-`` for stdin. SPDX and CycloneDX JSON are both accepted, and the format is
detected from the document itself.

.. code-block:: bash

   # From a file
   feluda --sbom-input sbom.spdx.json

   # From a pipe
   trivy image --format cyclonedx nginx:latest | feluda --sbom-input -

   # With any of the usual output modes and filters
   feluda --sbom-input sbom.cdx.json --json
   feluda --sbom-input sbom.cdx.json --restrictive --gist

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Flag
     - Description
   * - ``--sbom-input <FILE>``
     - SPDX or CycloneDX JSON document to analyse, or ``-`` for stdin
   * - ``--sbom-enriched <FILE>``
     - Write the input document back out with the licenses Feluda resolved

.. note::
   ``--sbom-input`` replaces the manifest scan, so the own-source and vendored passes do not run.
   There is no source tree behind the document. ``--path`` is still read, but only to detect the
   project license that compatibility is checked against. Use ``--project-license`` to state it
   directly.

----

How Components Are Read
-----------------------

Every component is identified by its PURL, which is what keeps a Debian ``libssl3`` distinct from
an npm package of the same name. The PURL also names the ecosystem, and that decides where a
missing license is looked up.

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Document
     - Where the license comes from
   * - SPDX
     - ``licenseConcluded``, falling back to ``licenseDeclared``. ``NOASSERTION`` and ``NONE`` count
       as unstated. A ``LicenseRef-`` id is resolved through the document's
       ``hasExtractedLicensingInfos``: the extracted license text is run through Feluda's content
       detector, so a reference becomes a canonical SPDX id wherever the text is recognisable.
   * - CycloneDX
     - ``licenses[].expression``, then ``licenses[].license.id``, then ``licenses[].license.name``.
       Several stated licenses are joined with ``AND``, because CycloneDX has no way to mark them
       as alternatives and a gate must not understate an obligation. The component in
       ``metadata.component`` is the artifact being described, not a dependency, so it is not
       reported.

----

Resolving NOASSERTION
---------------------

Cataloguing tools report ``NOASSERTION`` whenever a package's metadata carried no license, which on
a real image is a large share of the inventory. Feluda asks each package's own registry instead:
crates.io, npm, PyPI, Maven Central, RubyGems, NuGet, pkg.go.dev, R-universe and Conan Center.

.. code-block:: text

   pkg:npm/lodash@4.17.21    NOASSERTION  →  MIT

Components with no registry behind them (Debian, RPM and Alpine packages, and anything with no
PURL) keep whatever the document said. A document that already states every license needs no
lookups at all, and is analysed entirely offline.

----

Writing an Enriched Copy
------------------------

``--sbom-enriched`` writes the input document back out with Feluda's conclusions merged in.

.. code-block:: bash

   feluda --sbom-input sbom.spdx.json --sbom-enriched sbom.enriched.spdx.json

Only components Feluda actually resolved are touched; everything else is reproduced exactly as it
arrived. In SPDX the resolved license lands in ``licenseConcluded``, which is precisely the field
for a conclusion someone drew rather than something the package declared. A license that is not an
SPDX id or expression is written as a ``LicenseRef-feluda-*`` reference and defined in
``hasExtractedLicensingInfos``, so the result stays a valid document. In CycloneDX the component
gains a ``licenses`` entry, using ``expression`` for a compound license, ``id`` for an SPDX id, and
``name`` for anything else.

----

Gate a Pipeline
---------------

Everything in :ref:`cli-scan` works the same way here, including the exit codes.

.. code-block:: yaml

   - name: License gate on the shipped image
     run: |
       syft ghcr.io/org/app:${{ github.sha }} -o spdx-json > image.spdx.json
       feluda --sbom-input image.spdx.json \
              --project-license Apache-2.0 \
              --ci-format github \
              --fail-on-restrictive \
              --fail-on-incompatible

Next Steps
----------

- :ref:`sbom-validate`: check a document against its specification
- :ref:`cli-filter`: narrow the report before gating on it
