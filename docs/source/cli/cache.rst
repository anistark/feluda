:description: Feluda cache command for managing license cache.

.. _cli-cache:

cache
=====

.. rst-class:: lead

   View and manage Feluda's license cache to optimize scans and stay under rate limits.

----

Overview
--------

Feluda caches what it fetches, to stay under rate limits and to speed up repeated scans. Two files
live in your user cache directory:

.. list-table::
   :header-rows: 1
   :widths: 35 20 45

   * - File
     - Expires after
     - Holds
   * - ``github_licenses.json``
     - 30 days
     - The GitHub license table, which drives restrictiveness classification
   * - ``clearlydefined.json``
     - 7 days
     - Declared licenses ClearlyDefined answered with, keyed by package coordinate. Packages it had no answer for are cached too, so they are not asked about again on every run

----

View Cache Status
-----------------

Inspect cache statistics like size and freshness.

.. code-block:: bash

   feluda cache

Feluda prints each cache's location, its number of entries, and whether it is still valid.

**Output includes:**

- Cache file location
- Number of cached entries
- Cache age and validity status
- Last update timestamp

----

Clear the Cache
---------------

Remove stale or corrupted cache data.

.. code-block:: bash

   feluda cache --clear

Feluda deletes both cache files so the next scan starts fresh with remote data.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--clear``
     - Delete both cache files and start fresh

----

Cache Behavior
--------------

.. tip::
   Cache files older than 30 days refresh automatically, but explicit clears help when switching GitHub identities.

**When to clear the cache:**

- After changing GitHub tokens or identities
- When license data seems stale or incorrect
- After upgrading Feluda to a new version
- When debugging unexpected license detection results

**Cache location:**

Your user cache directory, which is ``~/Library/Caches/feluda`` on macOS,
``$XDG_CACHE_HOME/feluda`` (or ``~/.cache/feluda``) on Linux, and ``%LOCALAPPDATA%\feluda`` on
Windows. It is shared across projects, so a license resolved for one is already resolved for the
next.
