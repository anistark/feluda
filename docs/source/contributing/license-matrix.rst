:description: Maintaining the Feluda license compatibility matrix.

License Compatibility Matrix
============================

.. rst-class:: lead

   Guidelines for maintaining the license compatibility matrix.

----

The license compatibility matrix is a critical component that determines which dependency licenses are compatible with different project licenses. This matrix is stored in ``config/license_compatibility.toml`` and requires careful maintenance.

Understanding the Matrix Structure
----------------------------------

The compatibility matrix follows this TOML format:

.. code-block:: toml

   [PROJECT_LICENSE_NAME]
   compatible_with = [
       "dependency-license-1",
       "dependency-license-2",
       # ... more compatible licenses
   ]

Each section represents a project license, and the ``compatible_with`` array lists all dependency licenses that can be safely used with that project license.

Guidelines for Matrix Updates
-----------------------------

.. warning::

   **Legal Expertise Required**: Modifying license compatibility rules requires legal knowledge. Consider these guidelines:

1. **Research Thoroughly**:

   - Consult official license documentation
   - Review legal analyses from recognized authorities (FSF, OSI, etc.)
   - Check compatibility matrices from other trusted sources

2. **Conservative Approach**:

   - When in doubt, mark as incompatible rather than compatible
   - Legal liability is better avoided than remedied later

3. **Common License Relationships**:

   - **Permissive to Permissive**: Generally compatible (MIT, BSD, Apache-2.0)
   - **Permissive to Copyleft**: Generally compatible (can include MIT in GPL project)
   - **Copyleft to Permissive**: Generally incompatible (cannot include GPL in MIT project)
   - **Copyleft to Same Copyleft**: Usually compatible (GPL-3.0 with GPL-3.0)
   - **Copyleft to Different Copyleft**: Requires careful analysis

4. **Testing Changes**:

   .. code-block:: sh

      # Test with the ignored license compatibility test
      cargo test licenses::tests::test_is_license_compatible_mit_project -- --ignored

      # Run all tests to ensure no regressions
      cargo test

      # Test with real projects to validate changes
      feluda --project-license MIT --path /path/to/test/project

Adding New License Support
--------------------------

To add support for a new project license:

1. **Research the License**: Understand its permissions, conditions, and limitations

2. **Determine Compatibility**: Research which licenses are compatible

3. **Add to Matrix**: Add a new section in ``config/license_compatibility.toml``:

   .. code-block:: toml

      [NEW-LICENSE-1.0]
      compatible_with = [
          # List compatible dependency licenses based on legal research
      ]

4. **Update Normalization**: Add license variations to the ``normalize_license_id`` function in ``src/licenses.rs``:

   .. code-block:: rust

      id if id.contains("NEW-LICENSE") && id.contains("1.0") => "NEW-LICENSE-1.0".to_string(),

5. **Add Tests**: Include the new license in relevant test cases

6. **Document**: Update README.md to list the new supported license

Common License Compatibility Patterns
-------------------------------------

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Project License
     - Can Include Dependencies From
   * - **MIT/BSD/ISC**
     - Only permissive licenses (MIT, BSD, Apache, ISC, etc.)
   * - **Apache-2.0**
     - Permissive licenses (same as MIT)
   * - **GPL-3.0**
     - Most licenses (permissive + LGPL + GPL family)
   * - **GPL-2.0**
     - Permissive + LGPL + GPL-2.0 (NOT Apache-2.0)
   * - **AGPL-3.0**
     - Similar to GPL-3.0 plus AGPL
   * - **LGPL-3.0/2.1**
     - Limited compatibility (mainly permissive)
   * - **MPL-2.0**
     - Permissive + MPL

Review Process for Matrix Changes
---------------------------------

All changes to the license compatibility matrix require:

1. **Detailed explanation** in the PR description of:

   - Why the change is needed
   - Legal reasoning or sources consulted
   - Impact on existing compatibility decisions

2. **Maintainer review** by someone with legal expertise or license knowledge

3. **Testing** with real-world projects to ensure changes work as expected

4. **Documentation updates** reflecting the changes

Legal Disclaimer
----------------

Contributors modifying the license compatibility matrix acknowledge that:

- This is not legal advice and should not be treated as such
- Users are responsible for their own license compliance
- Maintainers and contributors provide no warranty regarding compatibility decisions
- Legal counsel should be consulted for important compliance decisions
