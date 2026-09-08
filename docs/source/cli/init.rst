:description: Set up Feluda in a project with a generated config file and pre-commit hook.

.. _cli-init:

Initialise a Project
====================

.. rst-class:: lead

   One command to get a policy file and a pre-commit gate in place.

----

Overview
--------

Feluda needs no configuration to run, so ``init`` is a convenience rather than a
prerequisite. What it does is write the two files most projects end up wanting anyway:
a ``.feluda.toml`` carrying the license policy, and a ``.pre-commit-config.yaml`` that
runs the scan before a commit lands.

.. code-block:: bash

   feluda init

It inspects the project first, so what it writes is not generic. Manifests in the tree
decide which ecosystems are reported as detected, and the project's own license is
detected and recorded in the generated config, so compatibility checking works on the
next run without you naming it.

----

Options
-------

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Behaviour
   * - ``-p``, ``--path <PATH>``
     - Project directory to initialise. Defaults to ``./``.
   * - ``--force``
     - Overwrite ``.feluda.toml`` and add the hook without prompting. Use it in scripts,
       where there is nobody to answer the questions.
   * - ``--no-pre-commit``
     - Write only ``.feluda.toml``. Use it when the repository manages hooks another way.

Without ``--force`` the command asks before touching anything that already exists, so
running it twice is safe.

----

What Gets Written
-----------------

``.feluda.toml`` holds the restrictive list, an empty ``ignore`` list to fill in, and a
``max_depth`` for transitive resolution. The custom license and ClearlyDefined blocks
are written commented out, as a pointer to what can be configured rather than as active
policy. See :ref:`configuration` for the full schema.

``.pre-commit-config.yaml`` gets a local hook that runs ``feluda --fail-on-restrictive``
on every commit, with ``always_run`` set, since a dependency can change without a
manifest appearing in the staged diff. If the file already exists, the hook is appended
to it rather than replacing what is there, and a hook Feluda has already added is not
added twice.

The generated hook calls ``feluda`` from ``PATH``, so contributors need it installed.
See :ref:`install`.

.. code-block:: bash

   feluda init
   pre-commit install

The second command is what actually activates the hook, and ``init`` prints it as a
next step rather than running it for you.

----

Editing the Result
------------------

The generated policy is a starting point, not a recommendation to keep as is. The
restrictive list is deliberately conservative, and ``MPL-2.0`` or ``EPL-2.0`` in
particular are file level copyleft that many projects are happy to ship. Take them out
if that is your position, rather than working around the report later.

To gate on compatibility with your own license as well as on restrictive licenses, add
``--fail-on-incompatible`` to the hook's ``args``.
