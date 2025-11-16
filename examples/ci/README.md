# CI/CD Integration Examples

This directory contains example configurations for integrating Feluda into various CI/CD systems.

## GitHub Actions

### Option 1: Using the Feluda Action (Recommended)

The recommended way to use Feluda in GitHub Actions is to use the published action. This provides the best integration with automatic badge updates and configurable outputs.

See the [GitHub Action README](../../ACTION-README.md) for complete documentation.

### Option 2: Standalone Workflow

If you prefer a standalone workflow without using the published action, see [`license-check.yml`](./license-check.yml) for an example that:
- Installs Feluda directly via Cargo
- Runs license checks with GitHub-compatible output format
- Updates README badge with results
- Supports manual workflow dispatch

To use this example, choose one of the following methods:

**Option A: Copy the file manually**
1. Copy the contents of [`license-check.yml`](./license-check.yml)
2. Create `.github/workflows/license-check.yml` in your repository and paste the contents
3. Adjust the configuration as needed for your project

**Option B: Download with wget**
1. Run the following command in your repository root:
   ```sh
   mkdir -p .github/workflows
   wget https://raw.githubusercontent.com/anistark/feluda/main/examples/ci/license-check.yml -O .github/workflows/license-check.yml
   ```
2. Adjust the configuration as needed for your project

## Jenkins

See [`Jenkinsfile`](./Jenkinsfile) for an example Jenkins pipeline that:
- Checks out your code
- Installs Feluda via Cargo
- Runs license checks with Jenkins-compatible output format (JUnit XML)
- Publishes results as JUnit test reports

To use this example:
1. Copy `Jenkinsfile` to the root of your repository
2. Configure your Jenkins job to use the Jenkinsfile
3. Adjust the configuration as needed for your project

## General CI/CD Integration

Feluda supports output formats for different CI systems:
- `--ci-format github`: GitHub Actions Workflow Commands
- `--ci-format jenkins`: JUnit XML format

For other CI/CD systems, use the standard text or JSON output formats:
- `--output plain` or no flag: Plain text output (default)
- `--output json`: JSON format for programmatic parsing

Key flags for CI integration:
- `--fail-on-restrictive`: Exit with non-zero status if restrictive licenses are found
- `--fail-on-incompatible`: Exit with non-zero status if incompatible licenses are found
- `--output-file <path>`: Write results to a file instead of stdout
