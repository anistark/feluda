:description: Explore Feluda's key features for license detection and compliance.

.. _features:

Features
========

.. rst-class:: lead

   Discover what makes **Feluda** a reliable license investigator for your dependencies.

----

Feluda combines the sharp observation of its namesake detective with the
precision of Rust to uncover licensing clues hidden within your project's
dependencies. Whether you're maintaining open-source libraries or auditing
enterprise codebases, Feluda ensures that every dependency tells its story
clearly.

Core Features
-------------

Dependency Parsing
^^^^^^^^^^^^^^^^^^

Scans your project to identify all declared dependencies and their licenses.
Feluda walks through manifest files, lock files, and package metadata to build
a complete picture of your dependency tree.

License Classification
^^^^^^^^^^^^^^^^^^^^^^

Categorizes each license as *permissive*, *restrictive*, or *unknown* for
easier risk assessment. This classification helps teams quickly identify
dependencies that may require legal review.

Compatibility Checks
^^^^^^^^^^^^^^^^^^^^

Evaluates license compatibility between dependencies and your project's
declared license. Feluda uses a comprehensive compatibility matrix to detect
conflicts before they become legal issues.

OSI Mapping
^^^^^^^^^^^

Maps licenses to their OSI (Open Source Initiative) approval status and allows
filtering by OSI-approved licenses. This helps ensure your project uses
well-recognized open source licenses.

Restriction Detection
^^^^^^^^^^^^^^^^^^^^^

Flags dependencies that impose limits on personal or commercial use. Restrictive
licenses like GPL-3.0 or AGPL-3.0 are clearly marked for review.

Conflict Detection
^^^^^^^^^^^^^^^^^^

Highlights dependencies whose licenses may conflict with your project's terms.
Get early warnings about incompatibilities before they affect your release.

----

Compliance & Reporting
----------------------

Compliance File Generation
^^^^^^^^^^^^^^^^^^^^^^^^^^

Automatically creates legal attribution files such as ``NOTICE`` and
``THIRD_PARTY_LICENSES``. These files satisfy attribution requirements for
most open source licenses.

SBOM Export
^^^^^^^^^^^

Generates a Software Bill of Materials (SBOM) in **SPDX 2.3** and
**CycloneDX v1.5** formats for security and compliance reporting. SBOMs
are increasingly required by enterprise customers and regulatory frameworks.

----

Output & Integration
--------------------

Multiple Output Formats
^^^^^^^^^^^^^^^^^^^^^^^

Provides results in **plain text**, **JSON**, **YAML**, or **TUI** formats.
A **gist mode** is also available for restrictive environments, producing a
single-line summary.

CI/CD Integration
^^^^^^^^^^^^^^^^^

Integrates seamlessly with **GitHub Actions** and **Jenkins** to automate
license compliance in your pipeline. Fail builds early when problematic
licenses are detected.

Verbose Analysis
^^^^^^^^^^^^^^^^

Enables a detailed, human-readable view of all discovered licenses and their
classifications. Debug mode provides step-by-step insight into the detection
process.

----

Performance & Caching
---------------------

Smart Caching
^^^^^^^^^^^^^

Feluda caches GitHub license responses to minimize API calls and stay under
rate limits. Cache automatically refreshes after 30 days.

Local-First Detection
^^^^^^^^^^^^^^^^^^^^^

By default, Feluda examines local manifest files before calling remote APIs,
making scans fast and reliable even without network access.
