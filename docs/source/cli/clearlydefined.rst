:description: How Feluda uses ClearlyDefined to resolve licenses no manifest, registry or license file could answer.

.. _cli-clearlydefined:

ClearlyDefined Fallback
=======================

.. rst-class:: lead

   The last thing Feluda tries before reporting a license as unknown.

----

Overview
--------

Every resolution path ends somewhere. A manifest may state no license, a registry may not carry
one, an installed wheel may ship no metadata, and the GitHub fallback only helps when Feluda knows
which repository to ask about. Those dependencies report as ``Unknown``, which in a compliance
report is the least useful answer there is.

`ClearlyDefined <https://clearlydefined.io>`_ is a Linux Foundation project that curates exactly
this gap: one definition per package coordinate, harvested by scancode, licensee and reuse, then
corrected by human curation. Feluda asks it about anything it could not resolve on its own.

The lookup runs on every scan source: a manifest scan, an ingested SBOM, and a cataloged
filesystem all reach it, and so do ``feluda sbom`` and ``feluda generate``.

.. code-block:: bash

   feluda                                    # unknowns go to ClearlyDefined
   feluda --no-clearlydefined                # ...or they do not

----

What Gets Asked
---------------

Only dependencies that are still unresolved after everything else, and only those whose ecosystem
ClearlyDefined indexes. Every question is a package coordinate, so a whole scan's worth of unknowns
is one request.

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Ecosystem
     - Coordinate
   * - Rust
     - ``crate/cratesio/-/serde/1.0.219``
   * - Node
     - ``npm/npmjs/-/lodash/4.17.21``, ``npm/npmjs/@babel/core/7.24.0``
   * - Python
     - ``pypi/pypi/-/requests/2.32.3``
   * - Java
     - ``maven/mavencentral/com.fasterxml.jackson.core/jackson-databind/2.15.2``
   * - Ruby
     - ``gem/rubygems/-/rails/7.1.0``
   * - .NET
     - ``nuget/nuget/-/Newtonsoft.Json/13.0.3``
   * - Go
     - ``go/golang/github.com%2fgorilla/mux/v1.8.1``

Nothing else is asked about. OS packages from a ``--filesystem`` scan are out because ClearlyDefined
does not harvest rpm or apk, and a deb revision carries an architecture suffix Feluda does not
record; CRAN and Conan are not supported by the service; and a vendored or own-source finding is a
path rather than a package. A dependency pinned to a range rather than a version (``^1.2.3``) is
also skipped, since a range is not something a definition can answer.

----

What Comes Back
---------------

The declared license, and only that. A definition also carries per-file scan results, but those
include the licenses of test fixtures and vendored code inside the package, and reporting one of
those as the package's license would be worse than reporting nothing. Definitions that declare
``NOASSERTION``, ``NONE`` or ``OTHER`` are treated as no answer.

A license that arrives this way is classified exactly like one from a manifest: restrictiveness,
OSI status, compatibility and the CI gates all apply, so ``--fail-on-restrictive`` fails on a GPL
dependency Feluda only learned about from ClearlyDefined.

----

Caching
-------

Answers are cached alongside the GitHub license table, in Feluda's cache directory, for seven days.
That is shorter than the 30 days the GitHub table gets, because curations land continuously.

Misses are cached too: a package ClearlyDefined has never heard of is not asked about again on
every run. ``feluda cache --clear`` clears both caches.

----

Turning It Off
--------------

The lookup is on by default. It only runs for dependencies that already failed to resolve, and it
is one batched request, so the cost is a fraction of the license fetching Feluda already does. It
does mean package names and versions leave the machine, so projects that must not talk to a third
party service turn it off:

.. code-block:: bash

   feluda --no-clearlydefined

.. code-block:: toml

   # .feluda.toml
   [clearlydefined]
   enabled = false

.. code-block:: bash

   export FELUDA_CLEARLYDEFINED_ENABLED=false

``endpoint`` points the lookup somewhere else, for a mirror, a proxy, or a self-hosted instance:

.. code-block:: toml

   [clearlydefined]
   endpoint = "https://clearlydefined.internal/definitions"

----

When It Fails
-------------

Silently, and by design. No network, a timeout, an error from the service: the dependency stays
unresolved, which is where it already was. A scan never fails because ClearlyDefined was
unreachable, and the wait is bounded, so a service that stops answering costs seconds rather than
holding up a build.

----

Next Steps
----------

- :ref:`cli-scan` for the resolution order this sits at the end of
- :ref:`configuration` for the rest of ``.feluda.toml``
- :ref:`cli-cache` for managing what has been cached
