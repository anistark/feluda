:description: Explore Feluda's key features and supported programming languages.

.. _features:

Features & Supported Languages
==============================

.. rst-class:: lead

   Discover what makes **Feluda** a reliable license investigator and which
   programming languages it speaks fluently.

----

Feluda combines the sharp observation of its namesake detective with the
precision of Rust to uncover licensing clues hidden within your project's
dependencies. Whether you're maintaining open-source libraries or auditing
enterprise codebases, Feluda ensures that every dependency tells its story
clearly.

Core Features
--------------

- **Dependency Parsing:**
  Scans your project to identify all declared dependencies and their licenses.

- **License Classification:**
  Categorizes each license as *permissive*, *restrictive*, or *unknown* for
  easier risk assessment.

- **Compatibility Checks:**
  Evaluates license compatibility between dependencies and your project's
  declared license.

- **OSI Mapping:**
  Maps licenses to their OSI (Open Source Initiative) approval status and allows
  filtering by OSI-approved licenses.

- **Restriction Detection:**
  Flags dependencies that impose limits on personal or commercial use.

- **Conflict Detection:**
  Highlights dependencies whose licenses may conflict with your project's terms.

- **Compliance File Generation:**
  Automatically creates legal attribution files such as ``NOTICE`` and
  ``THIRD_PARTY_LICENSES``.

- **SBOM Export:**
  Generates a Software Bill of Materials (SBOM) in SPDX format for improved
  security and compliance reporting.

- **Multiple Output Formats:**
  Provides results in **plain text**, **JSON**, or **TUI** formats.
  A **gist mode** is also available for restrictive environments, producing a
  single-line summary.

- **CI/CD Integration:**
  Integrates seamlessly with **GitHub Actions** and **Jenkins** to automate
  license compliance in your pipeline.

- **Verbose Analysis:**
  Enables a detailed, human-readable view of all discovered licenses and their
  classifications.

----

Supported Languages
--------------------

Feluda speaks multiple programming dialects fluently.

.. raw:: html

   <div style="display: flex; flex-wrap: wrap; gap: 24px; justify-content: center; align-items: center; margin-top: 1rem;">
     <img src="_static/icons/c.svg" alt="C" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/c++.svg" alt="C++" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/go.svg" alt="Go" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/python.svg" alt="Python" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/rust.svg" alt="Rust" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/javascript.svg" alt="JavaScript" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/typescript.svg" alt="JavaScript" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/nodejs.svg" alt="JavaScript" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/r.svg" alt="R" width="80" height="80" style="object-fit: contain;">
   </div>

----

Feluda currently supports projects written in these ecosystems:
C, C++, Go, Python, Rust, JavaScript / TypeScript / NodeJS and R

----

.. note::

   Additional language ecosystems are under development. If you'd like Feluda to
   support a specific language or build tool, contributions are welcome!

