:description: Feluda finds every dependency license in your project, flags what is risky, and proves compliance in CI.

.. _features:

Features
========

.. rst-class:: lead

   One binary. Every dependency. No license surprises on release day.

Your ``Cargo.toml`` says MIT. Your ``package.json`` says MIT. Three levels down the
tree, something says AGPL-3.0, and nobody finds out until a customer's legal team
does. Feluda takes that case: one command, a full read of your dependency tree, and
a straight answer about what you are actually shipping.

.. raw:: html

   <div class="proof-strip">
     <div class="proof-tile">
       <span class="proof-value">10</span>
       <span class="proof-label">Languages scanned</span>
     </div>
     <div class="proof-tile">
       <span class="proof-value">2</span>
       <span class="proof-label">SBOM standards emitted</span>
     </div>
     <div class="proof-tile">
       <span class="proof-value">3</span>
       <span class="proof-label">Lines to gate CI</span>
     </div>
     <div class="proof-tile">
       <span class="proof-value">0</span>
       <span class="proof-label">Config files required</span>
     </div>
   </div>

One command, one verdict
------------------------

Run it in any repo. Feluda reads the manifests, resolves the licenses, checks them
against your own, and tells you where you stand.

.. code-block:: console

   $ feluda --gist

   🦀 FELUDA GIST
   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
   │ Project License                │ MIT
   │ Total Dependencies Scanned     │ 435
   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
   │ Restrictive dependencies       │ ⚠️ 2
   │ Incompatible dependencies      │ ❌ 26
   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
   │ Recommendation                 │ ⚠️ NEEDS ATTENTION
   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

That is Feluda scanning its own repository. No project file to write first, no
account to create, no service to send your dependency tree to.

----

See what your manifests are hiding
----------------------------------

A manifest only describes what a package manager installed. Plenty of code arrives
some other way, and that is exactly the code nobody audits.

.. grid:: 1 1 3 3
   :gutter: 3

   .. grid-item-card:: :iconify:`lucide:package-search` Every ecosystem in the repo
      :class-card: glassmorphic
      :link: supported-languages
      :link-type: doc

      Rust, Go, Python, JavaScript, TypeScript, Node, Java, C, C++, R, .NET, and Ruby.
      Manifests and lock files both, so transitive dependencies are not a blind spot.

   .. grid-item-card:: :iconify:`lucide:scan-eye` Code that arrived another way
      :class-card: glassmorphic

      Source files carrying a foreign ``SPDX-License-Identifier`` header, packages sitting
      in ``vendor/`` or ``third_party/``, and stray directories with a ``LICENSE`` no
      manifest accounts for. The classic copy, paste, or AI generated case.

   .. grid-item-card:: :iconify:`lucide:folder-tree` Monorepos, in one report
      :class-card: glassmorphic

      Cargo, npm, pnpm, yarn, ``go.work``, and uv workspaces resolve into a single
      unified report, with every dependency attributed to the member that pulled it in.

----

Know which licenses actually matter
-----------------------------------

Four hundred dependencies is not a finding. Two problems in four hundred is a finding.
Feluda does the triage so you read a short list instead of a spreadsheet.

.. code-block:: console

   $ feluda --restrictive

   📄 Project License: MIT

   ⚠️ Warning: Restrictive licenses found!

   ┌────────────────────────────────┐
   │ Package    │ Version │ License │
   ├────────────────────────────────┤
   │ colored    │ 3.1.1   │ MPL-2.0 │
   │ option-ext │ 0.2.0   │ MPL-2.0 │
   └────────────────────────────────┘

.. grid:: 1 1 3 3
   :gutter: 3

   .. grid-item-card:: :iconify:`lucide:shield-alert` Risk classification
      :class-card: glassmorphic

      Every license lands in one of three buckets: permissive, restrictive, or unknown.
      ``--restrictive`` narrows the report to the ones that limit personal or commercial use.

   .. grid-item-card:: :iconify:`lucide:scale` Compatibility with your license
      :class-card: glassmorphic
      :link: contributing/license-matrix
      :link-type: doc

      Pass ``--project-license MIT`` and Feluda checks each dependency against a real
      compatibility matrix, then ``--incompatible`` shows only the conflicts.

   .. grid-item-card:: :iconify:`lucide:badge-check` OSI approval status
      :class-card: glassmorphic

      Filter on what the Open Source Initiative actually recognises with
      ``--osi approved``, ``--osi not-approved``, or ``--osi unknown``.

----

Ship the paperwork without writing it
-------------------------------------

Attribution files and SBOMs are the artifacts that turn "we checked" into something
you can hand to a customer, an auditor, or a procurement questionnaire.

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: :iconify:`lucide:file-text` NOTICE and THIRD_PARTY_LICENSES
      :class-card: glassmorphic

      ``feluda generate`` writes the attribution files most open source licenses
      require, populated from the dependencies actually in your tree.

   .. grid-item-card:: :iconify:`lucide:boxes` SPDX 2.3 and CycloneDX 1.5
      :class-card: glassmorphic
      :link: sbom/index
      :link-type: doc

      Both SBOM standards, straight out of the CLI, in the formats enterprise
      customers and regulators keep asking for.

   .. grid-item-card:: :iconify:`lucide:file-input` Read an SBOM back in
      :class-card: glassmorphic
      :link: sbom/ingest
      :link-type: doc

      Point Feluda at an SBOM someone else produced and scan it as a source, no
      original repository needed.

   .. grid-item-card:: :iconify:`lucide:check-check` Validate before you send it
      :class-card: glassmorphic
      :link: sbom/validate
      :link-type: doc

      ``feluda sbom validate`` checks an SBOM against its spec so you catch problems
      before your customer's tooling does.

.. code-block:: console

   $ feluda sbom spdx --output sbom.spdx.json

----

Fail the build, not the release
-------------------------------

The whole point is catching this in review, while the dependency is still easy to
swap. Three lines in a workflow, no install step.

.. code-block:: yaml

   - uses: anistark/feluda@v1
     with:
       fail-on-restrictive: true

.. grid:: 1 1 3 3
   :gutter: 3

   .. grid-item-card:: :iconify:`simple-icons:github` GitHub Action
      :class-card: glassmorphic
      :link: integrations/github-actions
      :link-type: doc

      Published on the Marketplace. Gate on restrictive licenses, incompatible
      licenses, or both, and keep a compliance badge current in your README.

   .. grid-item-card:: :iconify:`lucide:shield-check` SARIF for code scanning
      :class-card: glassmorphic

      ``--ci-format sarif`` emits SARIF 2.1.0, so findings land in GitHub Advanced
      Security and the VS Code problems panel next to everything else.

   .. grid-item-card:: :iconify:`simple-icons:jenkins` Jenkins
      :class-card: glassmorphic
      :link: integrations/jenkins
      :link-type: doc

      Pipeline ready, with exit codes that mean what you expect them to mean.

----

Fits the way you already work
-----------------------------

.. tab-set::
   :class: outline

   .. tab-item:: :iconify:`lucide:braces` JSON

      .. code-block:: json

         [
           {
             "name": "serde",
             "version": "1.0.151",
             "license": "MIT",
             "is_restrictive": false,
             "compatibility": "Compatible",
             "osi_status": "Approved",
             "ecosystem": "cargo",
             "purl": "pkg:cargo/serde@1.0.151"
           }
         ]

   .. tab-item:: :iconify:`lucide:file-code` YAML

      .. code-block:: yaml

         - name: serde
           version: 1.0.151
           license: MIT
           is_restrictive: false
           compatibility: Compatible
           osi_status: Approved
           ecosystem: cargo
           purl: pkg:cargo/serde@1.0.151

   .. tab-item:: :iconify:`lucide:terminal` Terminal UI

      .. code-block:: console

         $ feluda --gui

      A full terminal interface for browsing the dependency tree, sorting by risk,
      and reading each license without leaving the shell.

Every dependency carries its ecosystem and a PURL, so the output drops straight into
whatever you already pipe things through.

.. grid:: 1 1 3 3
   :gutter: 3

   .. grid-item-card:: :iconify:`lucide:zap` Local before network
      :class-card: glassmorphic

      Feluda reads manifests and installed packages on disk first, so scans stay fast
      and keep working when the network does not.

   .. grid-item-card:: :iconify:`lucide:database-zap` Caching that respects rate limits
      :class-card: glassmorphic
      :link: cli/cache
      :link-type: doc

      GitHub license lookups are cached for 30 days, so repeat scans stay quick and
      you stay well under the API limits.

   .. grid-item-card:: :iconify:`lucide:eye` Watch mode
      :class-card: glassmorphic
      :link: cli/watch
      :link-type: doc

      ``feluda watch`` rescans the moment a dependency file changes. Useful when an AI
      assistant is adding packages faster than you can review them.

----

Start investigating
-------------------

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: :iconify:`lucide:download` Install Feluda
      :class-card: glassmorphic
      :link: install
      :link-type: doc

      cargo, Homebrew, npm, pip, or a prebuilt binary. Pick one and run ``feluda``
      in your project.

   .. grid-item-card:: :iconify:`lucide:rocket` Two minute quickstart
      :class-card: glassmorphic
      :link: quickstart
      :link-type: doc

      From install to first verdict, with the flags worth knowing on day one.
