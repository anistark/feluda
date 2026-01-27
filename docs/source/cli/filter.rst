:description: Feluda filter options for narrowing scan results.

.. _cli-filter:

filter
======

.. rst-class:: lead

   Narrow scan results by language, OSI status, or license type.

----

Filter by Language
------------------

Mixed-language monorepos can overwhelm you with noise, so narrow the scope to the stack you're reviewing.

.. code-block:: bash

   feluda --language rust

Feluda shows entries only for the selected language while leaving the rest untouched for future scans.

**Supported values:**

.. list-table::
   :header-rows: 1
   :widths: 20 80

   * - Value
     - Ecosystem
   * - ``rust``
     - Rust (Cargo)
   * - ``node``
     - JavaScript / TypeScript / Node.js (npm)
   * - ``go``
     - Go (modules)
   * - ``python``
     - Python (pip, pipenv, poetry)
   * - ``c``
     - C (Conan)
   * - ``cpp``
     - C++ (Conan)
   * - ``dotnet``
     - .NET (NuGet)
   * - ``r``
     - R (CRAN)

----

Filter by OSI Status
--------------------

Compliance teams often care about whether a license is OSI approved, unknown, or unapproved.

**Show only OSI-approved licenses:**

.. code-block:: bash

   feluda --osi approved

Feluda trims the output to dependencies whose licenses the OSI has blessed.

**Show only non-approved licenses:**

.. code-block:: bash

   feluda --osi not-approved

Feluda highlights the packages that lack OSI approval, helping you escalate early.

**Show only unknown status:**

.. code-block:: bash

   feluda --osi unknown

Feluda prints only the entries with unknown OSI status so you can investigate manually.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Value
     - Description
   * - ``approved``
     - Licenses approved by the Open Source Initiative
   * - ``not-approved``
     - Licenses not approved by the OSI
   * - ``unknown``
     - Licenses with unknown OSI status

----

Filter by Restrictive Licenses
------------------------------

Feluda color-codes risk, but sometimes you just want the risky findings.

.. code-block:: bash

   feluda --restrictive

Feluda lists only the dependencies carrying licenses from your restrictive list or config (GPL-3.0, AGPL-3.0, MPL-2.0, etc.).

----

Filter by Incompatible Licenses
-------------------------------

Show every dependency that conflicts with your declared project license.

.. code-block:: bash

   feluda --incompatible

Feluda filters to dependencies whose licenses fail the compatibility matrix described in :ref:`configuration`.

----

Declare Project License
-----------------------

Check compatibility against a specific outbound license before redistribution.

.. code-block:: bash

   feluda --project-license MIT

Feluda compares every dependency against the MIT row in ``config/license_compatibility.toml`` and flags conflicts.

----

Strict Mode
-----------

Use strict mode when unknown licenses should be treated as incompatible.

.. code-block:: bash

   feluda --strict --project-license MIT

Feluda marks any dependency with an unrecognized license as incompatible, preventing ambiguous licenses from slipping through.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--strict``
     - Treat unknown licenses as incompatible
   * - ``--project-license <LICENSE>``
     - SPDX identifier of your project's license

----

Combining Filters
-----------------

Filters can be combined for precise results:

.. code-block:: bash

   # Rust dependencies with restrictive licenses
   feluda --language rust --restrictive

   # Python packages not OSI-approved
   feluda --language python --osi not-approved

   # All incompatible dependencies for an MIT project
   feluda --project-license MIT --incompatible --strict
