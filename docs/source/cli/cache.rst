:description: Feluda cache command for managing license cache.

.. _cli-cache:

cache
=====

.. rst-class:: lead

   View and manage Feluda's license cache to optimize scans and stay under rate limits.

----

Overview
--------

Feluda caches GitHub license responses under ``.feluda/cache/github_licenses.json`` to stay under rate limits and speed up repeated scans.

----

View Cache Status
-----------------

Inspect cache statistics like size and freshness.

.. code-block:: bash

   feluda cache

Feluda prints the cache location, the number of entries, and whether the cache is still valid.

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

Feluda deletes the cache file so the next scan starts fresh with remote data.

**Options:**

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Flag
     - Description
   * - ``--clear``
     - Delete the cache file and start fresh

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

The cache is stored at ``.feluda/cache/github_licenses.json`` relative to the scanned project directory.
