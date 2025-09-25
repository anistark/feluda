# Feluda License Scanner Action

A GitHub Action that scans your project dependencies for restrictive and incompatible licenses using [Feluda](https://github.com/anistark/feluda).

## Usage

### Basic Usage

```yaml
- name: Scan licenses
  uses: anistark/feluda@v1
```

### Advanced Usage

```yaml
- name: Scan licenses with custom settings
  uses: anistark/feluda@v1
  with:
    path: './my-project'
    fail-on-restrictive: true
    fail-on-incompatible: true
    project-license: 'MIT'
    update-badge: true
    badge-path: 'README.md'
```

## Inputs

| Name | Description | Required | Default |
|------|-------------|----------|---------|
| `path` | Path to the project directory to scan | No | `./` |
| `fail-on-restrictive` | Fail when restrictive licenses are found | No | `true` |
| `fail-on-incompatible` | Fail when incompatible licenses are found | No | `false` |
| `project-license` | Specify the project license (overrides auto-detection) | No | - |
| `update-badge` | Update README badge with scan results | No | `true` |
| `badge-path` | Path to README file for badge updates | No | `README.md` |

## Outputs

| Name | Description |
|------|-------------|
| `license-check` | Result of license check (`success`/`failure`) |
| `feluda-log` | Full output from Feluda scan |
| `restrictive-count` | Number of restrictive licenses found |
| `incompatible-count` | Number of incompatible licenses found |

## Examples

### Fail on Both Restrictive and Incompatible Licenses

```yaml
name: License Check
on: [push, pull_request]

jobs:
  license-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Check licenses
        uses: anistark/feluda@v1
        with:
          fail-on-restrictive: true
          fail-on-incompatible: true
```

### Custom Project License

```yaml
- name: Check licenses for Apache project
  uses: anistark/feluda@v1
  with:
    project-license: 'Apache-2.0'
    fail-on-incompatible: true
```

### Skip Badge Updates

```yaml
- name: Check licenses only
  uses: anistark/feluda@v1
  with:
    update-badge: false
```

### Use Outputs

```yaml
- name: Check licenses
  id: license-check
  uses: anistark/feluda@v1

- name: Comment on PR
  if: steps.license-check.outputs.license-check == 'failure'
  run: |
    echo "Found ${{ steps.license-check.outputs.restrictive-count }} restrictive licenses"
    echo "Found ${{ steps.license-check.outputs.incompatible-count }} incompatible licenses"
```

## Badge Integration

If `update-badge` is enabled, the action will automatically update your README badge:

```markdown
[![Scanned with Feluda](https://img.shields.io/badge/Scanned%20with-Feluda-brightgreen)](https://github.com/anistark/feluda)
```

The badge color changes based on scan results:
- 🟢 Green (`brightgreen`) - No issues found
- 🔴 Red (`red`) - Restrictive or incompatible licenses found

## What Does Feluda Check?

### Restrictive Licenses
Licenses with conditions like:
- `source-disclosure` (e.g., GPL, AGPL)
- `network-use-disclosure` (e.g., AGPL)

### Incompatible Licenses
Licenses that may be incompatible with your project's license based on compatibility matrices.

## Supported Languages

Feluda automatically detects and scans dependencies for:
- Rust (Cargo.toml)
- Node.js (package.json, package-lock.json, yarn.lock, pnpm-lock.yaml)
- Python (requirements.txt, Pipfile, pyproject.toml)
- Go (go.mod)
- C/C++ (conanfile.txt, conanfile.py, vcpkg.json, Makefile)

## License

This action is MIT licensed. See [LICENSE](LICENSE) for details.
