# R Example Project for Feluda

This is an example R package designed to test Feluda's license analysis capabilities for R projects.

## Dependencies

This project includes common R packages with various licenses:

- **dplyr** - Data manipulation package
- **ggplot2** - Data visualization
- **tidyr** - Data tidying tools
- **readr** - Reading rectangular data

## Project Files

- `DESCRIPTION` - Package metadata and dependencies (DCF format)
- `renv.lock` - Lockfile with specific package versions (JSON format)
- `R/example.R` - Sample R code using the dependencies

## Testing with Feluda

Run Feluda on this example project:

```sh
# From the repository root
feluda --path examples/r-example

# Verbose output
feluda --path examples/r-example --verbose

# JSON output
feluda --path examples/r-example --json

# Test with renv.lock file
feluda --path examples/r-example
```

## Expected Output

Feluda should detect and analyze licenses for:
- Direct dependencies (dplyr, ggplot2, tidyr, readr)
- Transitive dependencies (via renv.lock)
- License information fetched from R-universe API

## Notes

This example demonstrates:
- **DESCRIPTION file parsing** - Analyzes direct dependencies only
- **renv.lock file support** - Includes all transitive dependencies (already resolved by renv)
- **R-universe API integration** - Fetches license information for each package
- **Multi-field dependency detection** - Handles Imports, Depends, Suggests, LinkingTo fields

### Transitive Dependencies

- **renv.lock**: Contains the full dependency tree (direct + transitive). Feluda analyzes all packages listed.
- **DESCRIPTION**: Contains only direct dependencies. For complete transitive dependency analysis, use `renv.lock` or run `renv::snapshot()` to generate a lockfile.
