:description: Feluda generate command for creating compliance files.

.. _cli-generate:

generate
========

.. rst-class:: lead

   Create NOTICE and THIRD_PARTY_LICENSES files for legal attribution.

----

Overview
--------

Feluda guides you through interactive generation and scripted exports alike. These files provide the legal attribution required when redistributing software with third-party dependencies.

----

Interactive Mode
----------------

Let Feluda prompt you for which artifact to create.

.. code-block:: bash

   feluda generate

Feluda prompts for NOTICE or THIRD_PARTY_LICENSES and renders the chosen file in-place.

**Interactive options:**

1. ``NOTICE`` - Concise attribution summary
2. ``THIRD_PARTY_LICENSES`` - Full license texts

----

Generate with Options
---------------------

Target a specific ecosystem and outbound license without prompts.

.. code-block:: bash

   feluda generate --language rust --project-license MIT

Feluda limits its analysis to the requested language and projects compatibility calculations against MIT.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Flag
     - Description
   * - ``--language <LANG>``
     - Limit to a specific ecosystem
   * - ``--project-license <LICENSE>``
     - SPDX identifier for compatibility checks
   * - ``--path <PATH>``
     - Output directory for generated files

----

Specify Output Path
-------------------

Point Feluda elsewhere when compliance files must live in another workspace.

.. code-block:: bash

   feluda generate --path /opt/service

Feluda writes the selected artifacts into ``/opt/service`` with the relevant dependencies included.

----

Generated Files
---------------

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - File
     - Purpose
   * - ``NOTICE``
     - Concise attribution summaries for distribution
   * - ``THIRD_PARTY_LICENSES``
     - Full license texts for downstream consumers

.. note::
   Use ``NOTICE`` for concise attribution summaries and ``THIRD_PARTY_LICENSES`` when downstream consumers require the full license texts.

----

CI/CD Usage
-----------

Automate generation in pipelines:

.. code-block:: bash

   # Generate NOTICE (option 1)
   echo "1" | feluda generate

   # Generate THIRD_PARTY_LICENSES (option 2)
   echo "2" | feluda generate

   # Generate both
   echo "1" | feluda generate && echo "2" | feluda generate
