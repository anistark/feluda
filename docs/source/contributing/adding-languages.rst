:description: Guide to adding support for new programming languages in Feluda.

Adding Language Support
=======================

.. rst-class:: lead

   How to add support for new programming languages in Feluda.

----

Feluda follows a modular architecture for language support. Each language has its own module in ``src/languages/`` that implements dependency parsing and license resolution.

Language Module Structure
-------------------------

When adding a new language, create a new file ``src/languages/your_language.rs`` with this structure:

.. code-block:: rust

   use crate::config::FeludaConfig;
   use crate::debug::{log, LogLevel};
   use crate::licenses::LicenseInfo;
   use std::collections::{HashMap, HashSet};

   /// Analyze dependencies and their licenses for YourLanguage projects
   pub fn analyze_your_language_licenses(project_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
       // Implementation here
   }

   /// Parse direct dependencies from project files
   fn parse_direct_dependencies(project_path: &str, config: &FeludaConfig) -> Vec<(String, String)> {
       // Parse project files (package.json, Cargo.toml, etc.)
   }

   /// Resolve transitive dependencies with depth tracking
   fn resolve_transitive_dependencies(
       project_path: &str,
       direct_deps: &[(String, String)],
       max_depth: u32,
   ) -> Vec<(String, String)> {
       // Implement BFS traversal for transitive dependencies
       // Follow the pattern used in other language modules
   }

   /// Fetch license information for a specific dependency
   fn fetch_license_for_dependency(name: &str, version: &str) -> Option<String> {
       // Query package registries/APIs for license information
   }

Implementation Guidelines
-------------------------

1. **Follow Existing Patterns**: Study ``src/languages/rust.rs`` or ``src/languages/python.rs`` for reference implementation patterns.

2. **Transitive Dependency Resolution**: Implement BFS (Breadth-First Search) traversal with these features:

   - Cycle detection using ``HashSet<String>`` to track visited packages
   - Depth tracking with ``max_depth`` parameter from config
   - Progress tracking for large dependency trees

3. **Error Handling**: Use the debug logging system consistently:

   .. code-block:: rust

      use crate::debug::{log, LogLevel};

      if let Err(err) = some_operation() {
          log(LogLevel::Warn, &format!("Failed to fetch {}: {}", package_name, err));
      }

4. **Configuration Support**: Respect the ``max_depth`` configuration:

   .. code-block:: rust

      let max_depth = config.max_depth.unwrap_or(3);

5. **Package Manager Integration**: Connect to official package registries when possible:

   - Query official APIs for license information
   - Handle API rate limits and failures gracefully
   - Cache results when appropriate

Language Implementation Examples
--------------------------------

Different language ecosystems require different approaches. Here are some examples:

**C Module Features** (``src/languages/c.rs``):

- Autotools support (``configure.ac``, ``configure.in``)
- Makefile parsing
- pkg-config integration
- System package resolution

**C++ Module Features** (``src/languages/cpp.rs``):

- Modern package managers (vcpkg, Conan)
- Build system integration (CMake, Bazel)
- Package manager API queries
- Transitive dependency resolution

**R Module Features** (``src/languages/r.rs``):

- DESCRIPTION file parsing (DCF format)
- renv.lock support (JSON format)
- R-universe API integration for license fetching
- Handles Imports, Depends, Suggests, and LinkingTo fields

Updating Language Detection
---------------------------

After creating your language module, update ``src/languages/mod.rs``:

1. **Add the module**:

   .. code-block:: rust

      pub mod your_language;

2. **Add to exports**:

   .. code-block:: rust

      pub use your_language::analyze_your_language_licenses;

3. **Update Language enum**:

   .. code-block:: rust

      pub enum Language {
          YourLanguage(&'static str),
          // ... existing variants
      }

4. **Add file detection**:

   .. code-block:: rust

      impl Language {
          pub fn from_file_name(file_name: &str) -> Option<Self> {
              match file_name {
                  "your-project-file.ext" => Some(Language::YourLanguage("your-project-file.ext")),
                  // ... existing patterns
              }
          }
      }

5. **Update parser.rs**: Add parsing logic in ``src/parser.rs``:

   .. code-block:: rust

      match language {
          Language::YourLanguage(file_name) => {
              languages::analyze_your_language_licenses(project_path, config)
          }
          // ... existing cases
      }

Testing New Language Support
----------------------------

1. **Create test projects** in various scenarios
2. **Test transitive dependency resolution** with different depth configurations
3. **Validate license detection** accuracy
4. **Test error handling** for invalid/missing project files

Documentation Updates
---------------------

After implementing language support:

1. **Update README.md** with language badge and supported file types
2. **Add usage examples** specific to your language
3. **Document supported package managers** and build systems
4. **Update CLI help text** to include the new language filter
