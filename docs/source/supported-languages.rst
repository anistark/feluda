:description: Programming languages and ecosystems supported by Feluda.

.. _supported-languages:

Supported Languages
===================

.. rst-class:: lead

   Feluda speaks multiple programming dialects fluently.

----

.. raw:: html

   <div style="display: flex; flex-wrap: wrap; gap: 24px; justify-content: center; align-items: center; margin-top: 1rem;">
     <img src="_static/icons/c.svg" alt="C" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/c++.svg" alt="C++" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/go.svg" alt="Go" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/python.svg" alt="Python" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/rust.svg" alt="Rust" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/javascript.svg" alt="JavaScript" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/typescript.svg" alt="TypeScript" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/nodejs.svg" alt="Node.js" width="80" height="80" style="object-fit: contain;">
     <img src="_static/icons/r.svg" alt="R" width="80" height="80" style="object-fit: contain;">
   </div>

----

Language Support Matrix
-----------------------

Feluda currently supports projects written in these ecosystems:

.. list-table::
   :header-rows: 1
   :widths: 25 35 40

   * - Language
     - Manifest Files
     - Notes
   * - C
     - ``conanfile.txt``, ``conanfile.py``
     - Conan package manager
   * - C++
     - ``conanfile.txt``, ``conanfile.py``
     - Conan package manager
   * - Go
     - ``go.mod``, ``go.sum``
     - Go modules
   * - Python
     - ``requirements.txt``, ``Pipfile``, ``pyproject.toml``
     - pip, pipenv, poetry
   * - Rust
     - ``Cargo.toml``, ``Cargo.lock``
     - Cargo package manager
   * - JavaScript / TypeScript
     - ``package.json``, ``package-lock.json``
     - npm, yarn
   * - Node.js
     - ``package.json``, ``package-lock.json``
     - npm ecosystem
   * - .NET (C#/F#/VB)
     - ``*.csproj``, ``*.fsproj``, ``packages.config``
     - NuGet packages
   * - R
     - ``DESCRIPTION``, ``renv.lock``
     - CRAN packages

----

Language-Specific Scanning
--------------------------

Filter scans to a specific ecosystem using the ``--language`` flag:

.. code-block:: bash

   feluda --language rust
   feluda --language python
   feluda --language go
   feluda --language node
   feluda --language c
   feluda --language cpp
   feluda --language dotnet
   feluda --language r

----

.. note::

   Additional language ecosystems are under development. If you'd like Feluda to
   support a specific language or build tool, contributions are welcome!
