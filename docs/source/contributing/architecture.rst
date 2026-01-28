:description: Feluda internal architecture - caching, OSI integration, and error handling.

Architecture
============

.. rst-class:: lead

   Understanding Feluda's internal architecture for contributors.

----

Error Handling
--------------

Feluda uses a custom error type for consistent error handling. When adding new code, use the ``FeludaError`` and ``FeludaResult`` types:

.. code-block:: rust

   // Return a Result with a specific error type
   fn my_function() -> FeludaResult<MyType> {
       match some_operation() {
           Ok(result) => Ok(result),
           Err(err) => Err(FeludaError::Parser(format!("Operation failed: {}", err)))
       }
   }

Available Error Types
^^^^^^^^^^^^^^^^^^^^^

The ``FeludaError`` enum provides specific error variants for different error scenarios. Use the most specific error type that matches your situation:

.. list-table::
   :header-rows: 1
   :widths: 20 30 50

   * - Error Variant
     - Use Case
     - Example
   * - ``Io(std::io::Error)``
     - File system operations, I/O errors
     - File read/write failures (auto-converted via ``From`` trait)
   * - ``Http(reqwest::Error)``
     - Network requests, API calls
     - HTTP client errors (auto-converted via ``From`` trait)
   * - ``Config(String)``
     - Configuration loading/validation
     - Invalid config values, missing required settings
   * - ``License(String)``
     - License analysis, compatibility checks
     - Invalid license format, compatibility violations
   * - ``Parser(String)``
     - Dependency file parsing
     - Malformed package.json, invalid Cargo.toml
   * - ``RepositoryClone(String)``
     - Git repository cloning
     - Clone failures, authentication issues
   * - ``TempDir(String)``
     - Temporary directory operations
     - Failed to create or access temp directories
   * - ``TuiInit(String)``
     - TUI initialization
     - Terminal setup failures, color_eyre errors
   * - ``TuiRuntime(String)``
     - TUI runtime operations
     - Runtime errors during TUI execution
   * - ``Serialization(String)``
     - JSON/YAML serialization
     - Failed to serialize SBOM documents
   * - ``FileWrite(String)``
     - File write operations
     - Failed to write SBOM or license files
   * - ``InvalidData(String)``
     - Data validation
     - Malformed SPDX data, invalid characters
   * - ``Unknown(String)``
     - Fallback for uncategorized errors
     - Use only when no specific type fits

**Guidelines:**

- Prefer specific error types over ``Unknown``
- Include context in error messages: ``FeludaError::Parser(format!("Failed to parse {}: {}", file, err))``
- Use ``map_err()`` to convert errors: ``.map_err(|e| FeludaError::Serialization(format!("Failed to serialize: {e}")))?``
- ``Io`` and ``Http`` errors are auto-converted via the ``From`` trait, no manual conversion needed

Cache Architecture
------------------

Feluda implements a multi-tier caching strategy to improve performance on repeated analyses. Currently, GitHub License Cache is implemented.

GitHub License Cache
^^^^^^^^^^^^^^^^^^^^

This caches the GitHub Licenses API data to avoid repeated network requests.

**Implementation Details:**

- **Storage**: ``~/.feluda/cache/github_licenses.json``
- **TTL**: 30 days (configurable via ``CACHE_TTL_SECS`` constant)
- **Size**: ~5-10 KB for typical GitHub license database
- **Files**:

  - ``src/cache.rs``: Core caching module
  - ``src/licenses.rs``: Integration point (load/save)
  - ``src/main.rs``: CLI command handler

**Key Functions** (``src/cache.rs``):

.. code-block:: rust

   pub fn load_github_licenses_from_cache() -> FeludaResult<Option<HashMap<String, License>>>
   pub fn save_github_licenses_to_cache(licenses: &HashMap<String, License>) -> FeludaResult<()>
   pub fn get_cache_status() -> FeludaResult<CacheStatus>
   pub fn clear_github_licenses_cache() -> FeludaResult<()>

**CacheStatus Structure**:

.. code-block:: rust

   pub struct CacheStatus {
       pub exists: bool,           // Whether cache file exists
       pub path: PathBuf,          // Full cache file path
       pub size_bytes: u64,        // Cache file size in bytes
       pub is_fresh: bool,         // Whether cache is within TTL
       pub age_secs: u64,          // Cache age in seconds
       pub license_count: usize,   // Number of licenses cached
   }

**CLI Integration**:

.. code-block:: sh

   # View cache status
   feluda cache

   # Clear cache
   feluda cache --clear

**Performance Impact**:

- First run: Full GitHub API call (~30-60 seconds depending on network)
- Subsequent runs (cache hit): Instant license loading
- Cache miss/stale: Falls back to GitHub API automatically
- Typical speedup: 50-100x faster for analyses within 30 days

Future Considerations (TODO)
^^^^^^^^^^^^^^^^^^^^^^^^^^^^

**Per-Package License Cache**

- Cache individual package license lookups
- Key: ``{language}:{package_name}:{version}``
- Useful for monorepos with repeated dependencies
- Storage: ``~/.feluda/cache/packages.json``
- Example: npm/Rust packages with same versions

**Dependency Manifest Cache**

- Cache parsed dependency lists with mtime tracking
- Skips re-parsing unchanged manifest files
- Requires careful invalidation logic
- Useful for frequent CI/CD runs on same project

Testing Cache Implementation
^^^^^^^^^^^^^^^^^^^^^^^^^^^^

When working with cache functionality:

.. code-block:: sh

   # Test cache status display
   cargo run -- cache

   # Clear cache before testing
   cargo run -- cache --clear

   # Verify cache is created after analysis
   cargo run -- --path examples/rust-example
   cargo run -- cache

   # Verify cache is used on second run
   cargo run -- --path examples/rust-example --debug 2>&1 | grep -i cache

Cache Debugging
^^^^^^^^^^^^^^^

Enable debug mode to see cache operations:

.. code-block:: sh

   feluda --debug 2>&1 | grep -i cache
   # Output will show:
   # - Cache hit/miss
   # - Cache age and freshness
   # - Save/load operations

OSI Integration
---------------

Feluda integrates with the Open Source Initiative (OSI) to provide license approval status information. The OSI integration consists of several components that work together to fetch, cache, and display OSI approval status for licenses.

OSI Integration Components
^^^^^^^^^^^^^^^^^^^^^^^^^^

1. **OSI API Integration** (``src/licenses.rs``):

   - ``fetch_osi_licenses()``: Fetches approved licenses from `OSI API <https://api.opensource.org/licenses/>`_
   - ``OsiLicenseInfo`` struct: Represents OSI license data structure
   - Concurrent HTTP requests with tokio for performance
   - Handles API failures gracefully with fallback mechanisms

2. **OSI Status Management**:

   - ``OsiStatus`` enum: ``Approved``, ``NotApproved``, ``Unknown``
   - ``get_osi_status()``: Maps SPDX license IDs to OSI approval status
   - Static fallback mappings for common licenses when API is unavailable
   - Integration in all language parsers to include OSI status in ``LicenseInfo``

3. **Display Integration**:

   - OSI status column in verbose table mode (``src/table.rs``)
   - OSI status in JSON/YAML output formats (``src/reporter.rs``)
   - Color-coded OSI status display in TUI mode
   - CLI filtering with ``--osi`` flag (``src/cli.rs``)

Modifying OSI Integration
^^^^^^^^^^^^^^^^^^^^^^^^^

When working with OSI integration:

**Adding New Static Mappings**: Update ``get_osi_status()`` in ``src/licenses.rs``:

.. code-block:: rust

   pub fn get_osi_status(license_id: &str, osi_licenses: &[OsiLicenseInfo]) -> OsiStatus {
       // Add new static mappings here for licenses not in OSI API
       match license_id {
           "NEW-LICENSE-ID" => OsiStatus::Approved, // If you know it's OSI approved
           // ... existing mappings
       }
   }

**Testing OSI Integration**:

.. code-block:: sh

   # Test OSI API connectivity and filtering
   cargo run -- --osi approved --verbose

   # Test fallback behavior (when API fails)
   # Temporarily break API URL in code and test

   # Test JSON output includes osi_status field
   cargo run -- --json | jq '.[0].osi_status'
