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

Contributors ✨
---------------

Thanks to all the people who contribute to Feluda!

.. raw:: html

   <div id="contributors-container">Loading contributors...</div>


.. toctree::
   :maxdepth: 1
   :caption: Introduction
   :hidden:

   self

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
