:description: Guide to releasing new versions of Feluda.

Releasing
=========

.. rst-class:: lead

   How to release new versions of Feluda.

----

This is only if you have release permissions. If not, contact the maintainers to get it.

Setup Requirements
------------------

- Install the gh CLI:

  .. code-block:: sh

     brew install gh     # macOS
     sudo apt install gh # Ubuntu/Debian

- Authenticate the gh CLI with GitHub:

  .. code-block:: sh

     gh auth login

We'll be using justfile for next steps, so setup `just <https://github.com/casey/just>`_ before proceeding.

Release Process
---------------

The release process is split into two steps:

Step 1: Publish to crates.io and push tag
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

.. code-block:: sh

   just publish                # Stable release (v1.0.0)
   just publish alpha          # Alpha release (v1.0.0-alpha)
   just publish beta           # Beta release (v1.0.0-beta)
   just publish rc-1           # Release candidate (v1.0.0-rc-1)

This will:

1. Build the release version
2. Test the release build
3. Create and validate the package
4. Publish to crates.io
5. Create and push the version tag to GitHub (with optional pre-release suffix)

Step 2: Create GitHub release manually
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

After the tag is pushed, create the GitHub release manually via the GitHub UI:

1. Go to https://github.com/anistark/feluda/releases
2. Click "Draft a new release"
3. Select the tag that was just pushed
4. Add release notes describing the changes
5. Publish the release

The ``release-binaries.yml`` workflow will automatically trigger on the release publish and build RPM and DEB packages, uploading them to the release.

Helper Commands
---------------

**Test the Release Build**

.. code-block:: sh

   just test-release

**Clean Artifacts**

.. code-block:: sh

   just clean

**Login to crates.io**

.. code-block:: sh

   just login
