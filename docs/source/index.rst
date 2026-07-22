.. Feluda documentation master file, created by
   sphinx-quickstart on Thu Sep 25 15:50:04 2025.
   You can adapt this file completely to your liking, but it should at least
   contain the root `toctree` directive.

👋 Feluda
===========

.. rst-class:: lead

   Feluda is the calm, methodical detective who surveys every dependency so you can ship with confidence.

Feluda, named after Satyajit Ray's iconic sleuth, now prowls through source trees instead of Calcutta's alleys.
He spots inconsistencies across Rust, Node, Go, Python, C/C++, .NET, and R stacks while keeping the story tidy.
You remain the trusted companion who decides when to dig deeper, escalate findings, or simply nod in approval.

Feluda's satchel carries scanning commands, filters, SBOM builders, cache tools, and CI integrations covered throughout this guide.
Each section ends with practical next steps so you can move from manual spot checks to automated compliance quickly.

.. grid:: 1 1 2 2
   :gutter: 3

   .. grid-item-card:: 🚀 Install Feluda
      :class-card: glassmorphic
      :link: install
      :link-type: doc

      Get started with Feluda in minutes. Install via cargo, npm, pip, or download binaries.

   .. grid-item-card:: 📦 Feluda Crate
      :class-card: glassmorphic
      :link: https://crates.io/crates/feluda
      :link-type: url

      View Feluda on crates.io - the Rust package registry.

   .. grid-item-card:: ⚡ GitHub Action
      :class-card: glassmorphic
      :link: integrations/github-actions
      :link-type: doc

      Gate CI on license compliance in 3 lines, no install needed. Available on the GitHub Marketplace.

   .. grid-item-card:: 🤝 Contribute to Feluda
      :class-card: glassmorphic
      :link: contributing/index
      :link-type: doc

      Join the community! Learn how to contribute code, docs, or report issues.

   .. grid-item-card:: 💻 CLI Reference
      :class-card: glassmorphic
      :link: cli/index
      :link-type: doc

      Explore all Feluda commands - scan, filter, cache, generate, and more.

.. warning:: **Legal Disclaimer**

   Feluda is provided as a helpful tool for license compliance analysis. However, it is **not a substitute for legal advice**, and users are responsible for their own compliance decisions:

   - **Verification**: You must verify the accuracy of all license information provided by Feluda
   - **Your Responsibility**: Ensure compliance with all applicable license terms and regulations
   - **Legal Counsel**: Always consult qualified legal counsel for license compliance matters
   - **Official Sources**: Check official repositories for up-to-date and authoritative license information
   - **No Warranty**: Feluda and its contributors provide no warranties regarding accuracy or fitness for any purpose
   - **No Liability**: Feluda and its contributors are not liable for any legal issues arising from the use of this tool or information
   - **Complexity**: License compatibility can depend on specific use cases, distribution methods, and jurisdictions

   Feluda is in active development. While we strive to provide accurate information, **use at your own risk.**

Talk to Us 💬
-------------

Using Feluda in your project or company? The roadmap is shaped by real investigations, and every case report counts.
Even a quick "we use Feluda for X" note helps shape what gets built next.

.. grid:: 1 1 3 3
   :gutter: 3

   .. grid-item-card:: 💬 Join the Discord
      :class-card: glassmorphic
      :link: https://discord.gg/AJMEeFXxXy
      :link-type: url

      Hang out in the Null Order server and tell us how you use Feluda.

   .. grid-item-card:: 🐛 Open an Issue
      :class-card: glassmorphic
      :link: https://github.com/anistark/feluda/issues/new
      :link-type: url

      Bug, feature request, or a case report. The tracker shapes the roadmap.

   .. grid-item-card:: ✉️ DM the Maintainers
      :class-card: glassmorphic

      .. raw:: html

         <div class="maintainer-handles">
           <div class="maintainer-row">
             <span class="maintainer-name">Anirudha</span>
             <a class="handle-btn" href="https://x.com/kranirudha" aria-label="Anirudha on X" title="Anirudha on X">
               <iconify-icon icon="simple-icons:x" aria-hidden="true"></iconify-icon>
             </a>
             <a class="handle-btn" href="https://fosstodon.org/@ani" aria-label="Anirudha on Mastodon" title="Anirudha on Mastodon">
               <iconify-icon icon="simple-icons:mastodon" aria-hidden="true"></iconify-icon>
             </a>
           </div>
           <div class="maintainer-row">
             <span class="maintainer-name">Farhaan</span>
             <a class="handle-btn" href="https://x.com/fhackdroid" aria-label="Farhaan on X" title="Farhaan on X">
               <iconify-icon icon="simple-icons:x" aria-hidden="true"></iconify-icon>
             </a>
             <a class="handle-btn" href="https://mastodon.social/@fhackdroid" aria-label="Farhaan on Mastodon" title="Farhaan on Mastodon">
               <iconify-icon icon="simple-icons:mastodon" aria-hidden="true"></iconify-icon>
             </a>
           </div>
         </div>

Contributors ✨
---------------

Thanks to all the people who contribute to Feluda!

.. raw:: html

   <div id="contributors-container">Loading contributors...</div>

Field Reports 🔎
----------------

Talks, videos, slides, and upcoming appearances where Feluda steps out of the codebase.
Have a session to share? `Open a PR <https://github.com/anistark/feluda/edit/main/docs/source/_static/field-reports.json>`_ to add it.

.. raw:: html

   <div id="field-reports-container">Loading field reports...</div>


.. toctree::
   :maxdepth: 1
   :caption: Introduction
   :hidden:

   👋 Feluda <self>

.. toctree::
   :maxdepth: 1
   :caption: Getting Started
   :hidden:

   install
   quickstart
   features
   supported-languages

.. toctree::
   :maxdepth: 2
   :caption: Feluda CLI
   :hidden:

   cli/index
   cli/scan
   cli/watch
   cli/filter
   cli/cache
   cli/generate
   cli/output

.. toctree::
   :maxdepth: 2
   :caption: SBOM
   :hidden:

   sbom/index
   sbom/spdx
   sbom/cyclonedx
   sbom/validate

.. toctree::
   :maxdepth: 2
   :caption: Integrations
   :hidden:

   integrations/index
   integrations/github-actions
   integrations/jenkins
   integrations/claude-code

.. toctree::
   :maxdepth: 1
   :caption: Configuration
   :hidden:

   configuration
   reference

.. toctree::
   :maxdepth: 2
   :caption: Development
   :hidden:

   contributing/index
   contributing/setup
   contributing/testing
   contributing/architecture
   contributing/license-matrix
   contributing/adding-languages
   contributing/releasing
