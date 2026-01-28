:description: Programming languages and ecosystems supported by Feluda.

.. _supported-languages:

Supported Languages
===================

.. rst-class:: lead

   Feluda speaks multiple programming dialects fluently.

----

.. raw:: html

   <style>
     .lang-icons {
       display: grid;
       grid-template-columns: repeat(6, 1fr);
       gap: 32px 24px;
       justify-items: center;
       align-items: center;
       margin: 1.5rem auto;
       max-width: 600px;
     }
     .lang-icons img {
       width: 64px;
       height: 64px;
       object-fit: contain;
     }
     @media (max-width: 768px) {
       .lang-icons {
         grid-template-columns: repeat(4, 1fr);
         gap: 24px 20px;
         max-width: 400px;
       }
       .lang-icons img {
         width: 60px;
         height: 60px;
       }
     }
     @media (max-width: 480px) {
       .lang-icons {
         grid-template-columns: repeat(3, 1fr);
         gap: 20px 16px;
         max-width: 280px;
       }
       .lang-icons img {
         width: 56px;
         height: 56px;
       }
     }
   </style>
   <div class="lang-icons">
     <img src="../_static/icons/rust.png" alt="Rust">
     <img src="../_static/icons/go.svg" alt="Go">
     <img src="../_static/icons/python.svg" alt="Python">
     <img src="../_static/icons/javascript.svg" alt="JavaScript">
     <img src="../_static/icons/typescript.svg" alt="TypeScript">
     <img src="../_static/icons/nodejs.svg" alt="Node.js">
     <img src="../_static/icons/c.svg" alt="C">
     <img src="../_static/icons/c++.svg" alt="C++">
     <img src="../_static/icons/r.svg" alt="R">
     <img src="../_static/icons/dotnet.svg" alt=".NET">
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
   * - Rust
     - ``Cargo.toml``, ``Cargo.lock``
     - Cargo package manager
   * - Go
     - ``go.mod``, ``go.sum``
     - Go modules
   * - Python
     - ``requirements.txt``, ``Pipfile``, ``pyproject.toml``
     - pip, pipenv, poetry
   * - JavaScript / TypeScript
     - ``package.json``, ``package-lock.json``
     - npm, pnpm, yarn, bun
   * - Node.js
     - ``package.json``, ``package-lock.json``
     - npm, pnpm, yarn, bun
   * - C
     - ``conanfile.txt``, ``conanfile.py``
     - Conan package manager
   * - C++
     - ``conanfile.txt``, ``conanfile.py``
     - Conan package manager
   * - R
     - ``DESCRIPTION``, ``renv.lock``
     - CRAN packages
   * - .NET (C#/F#/VB)
     - ``*.csproj``, ``*.fsproj``, ``packages.config``
     - NuGet packages

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

Coming Soon
-----------

- `Java <https://github.com/anistark/feluda/issues/54>`_
- `Ruby <https://github.com/anistark/feluda/issues/53>`_

----

.. note::

   Additional language ecosystems are under development. If you'd like Feluda to
   support a specific language or build tool, contributions are welcome!
