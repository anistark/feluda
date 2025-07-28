# Feluda

[![Crates.io Version](https://img.shields.io/crates/v/feluda)
](https://crates.io/crates/feluda) [![Crates.io Downloads](https://img.shields.io/crates/d/feluda)](https://crates.io/crates/feluda) [![Crates.io Downloads (latest version)](https://img.shields.io/crates/dv/feluda)](https://crates.io/crates/feluda) [![Open Source](https://img.shields.io/badge/open-source-brightgreen)](https://github.com/anistark/feluda) [![Contributors](https://img.shields.io/github/contributors/anistark/feluda)](https://github.com/anistark/feluda/graphs/contributors) ![maintenance-status](https://img.shields.io/badge/maintenance-actively--developed-brightgreen.svg) [![Scanned with Feluda](https://img.shields.io/badge/Scanned%20with-Feluda-red)](https://github.com/anistark/feluda)

🔎 **Feluda** is a Rust-based command-line tool that analyzes the dependencies of a project, notes down their licenses, and flags any permissions that restrict personal or commercial usage or are incompatible with your project's license.

![ss](https://github.com/user-attachments/assets/473908eb-43cb-4c4f-86aa-017de251afa8)

> 👋 It's still highly experimental, but fast iterating. Welcoming contributors and support to help bring out this project even better!

## Features

- Parse your project to identify dependencies and their licenses.
- Classify licenses into permissive, restrictive, or unknown categories.
- Check license compatibility between dependencies and your project's license.
- Flag dependencies with licenses that may restrict personal or commercial use.
- Flag dependencies with licenses that may be incompatible with your project's license.
- Generate compliance files (NOTICE and THIRD_PARTY_LICENSES) for legal requirements.
- Output results in plain text, JSON or TUI formats. There's also a gist format which is available in strict mode to output a single line only.
- CI/CD support for Github Actions and Jenkins.
- Verbose mode gives an enhanced view of all licenses.

### Support Languages

1. ![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
2. ![TypeScript](https://img.shields.io/badge/typescript-%23007ACC.svg?style=for-the-badge&logo=typescript&logoColor=white) ![JavaScript](https://img.shields.io/badge/javascript-%23323330.svg?style=for-the-badge&logo=javascript&logoColor=%23F7DF1E) ![NodeJS](https://img.shields.io/badge/node.js-6DA55F?style=for-the-badge&logo=node.js&logoColor=white)
3. ![Go](https://img.shields.io/badge/go-%2300ADD8.svg?style=for-the-badge&logo=go&logoColor=white)
4. ![Python](https://img.shields.io/badge/python-3670A0?style=for-the-badge&logo=python&logoColor=ffdd54)

Feluda supports analyzing dependencies across multiple languages simultaneously.

```sh
feluda
```

You can also filter the analysis to a specific language using the `--language` flag.
_If your fav language or framework isn't supported, feel free to open an feature request issue! 👋_

## Installation

### Official Distribution 🎉:

<details>
<summary>Rust (Crate)</summary>

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) installed on your system.

If you already had it, make sure it's up-to-date and update if needed.
(Optional) Set rust path if not set already.

### Install

```sh
cargo install feluda
```

</details>

### Community Maintained 🙌:

<details>
<summary>Homebrew (maintained by <a href="https://github.com/chenrui333" rel="noopener noreferrer">@chenrui333</a>)</summary>

![macOS](https://img.shields.io/badge/mac%20os-000000?style=for-the-badge&logo=macos&logoColor=F0F0F0) 

[feluda](https://formulae.brew.sh/formula/feluda) is available in the [Homebrew](https://formulae.brew.sh/).
You can install it using brew:

```sh
brew install feluda
```

</details>

<details>
<summary>Arch Linux (maintained by <a href="https://github.com/adamperkowski" rel="noopener noreferrer">@adamperkowski</a>)</summary>

![Arch](https://img.shields.io/badge/Arch%20Linux-1793D1?logo=arch-linux&logoColor=fff&style=for-the-badge)

[feluda](https://aur.archlinux.org/packages/feluda) is available in the [AUR](https://aur.archlinux.org/).
You can install it using an AUR helper (e.g. paru):

```sh
paru -S feluda
```

</details>

<details>
<summary>NetBSD (maintained by <a href="https://github.com/0323pin" rel="noopener noreferrer">@0323pin</a>)</summary>

![Linux](https://img.shields.io/badge/Linux-FCC624?style=for-the-badge&logo=linux&logoColor=black) 

On NetBSD a package is available from the [official repositories](https://pkgsrc.se/devel/feluda/). To install it, simply run:

```sh
pkgin install feluda
```

</details>

[![Packaging status](https://repology.org/badge/vertical-allrepos/feluda.svg)](https://repology.org/project/feluda/versions)

Track releases on [github releases](https://github.com/anistark/feluda/releases) or [via release feed](https://github.com/anistark/feluda/releases.atom).

<details>
<summary>Build from Source (advanced users)</summary>

**Note:** This might have experimental features which might not work as intended.

### Clone and Build

First, clone the repository:

```sh
git clone https://github.com/anistark/feluda.git
cd feluda
```

Then, build the project using Cargo:

```sh
cargo build --release
```

Finally, to make `feluda` available globally, move the binary to a directory in your PATH. For example:

```sh
sudo mv target/release/feluda /usr/local/bin/
```

</details>

## Usage

Feluda provides license analysis by default, with an additional command for generating compliance files.
Analyze your project's dependencies and their licenses:

```sh
# Basic usage
feluda

# Specify a path to your project directory
feluda --path /path/to/project/

# Check with specific language
feluda --language {rust|node|go|python}
```

### License File Generation

Generate compliance files for legal requirements:

```sh
# Interactive file generation
feluda generate

# Generate for specific language and license
feluda generate --language rust --project-license MIT

# Generate for specific path
feluda generate --path /path/to/project/
```

![generate-ss](https://github.com/user-attachments/assets/a965843f-7d87-4ba8-a311-c982d717a4f8)


### Run feluda on a github repo directly

```sh
feluda --repo <repository_url> [--ssh-key <key_path>] [--ssh-passphrase <passphrase>] [--token <https_token>]
```

` <repository_url>: The URL of the Git repository to clone (e.g., git@github.com:user/repo.git or https://github.com/user/repo.git). `

` --ssh-key <key_path>: (Optional) Path to a private SSH key for authentication. `

` --ssh-passphrase <passphrase>: (Optional) Passphrase for the SSH key. `

` --token <https_token>: (Optional) HTTPS token for authenticating with private repositories. `

_If you're using Feluda, feel free to grab a Scanned with Feluda badge for your project:_ [![Scanned with Feluda](https://img.shields.io/badge/Scanned%20with-Feluda-red)](https://github.com/anistark/feluda)
```
[![Scanned with Feluda](https://img.shields.io/badge/Scanned%20with-Feluda-red)](https://github.com/anistark/feluda)
```

## License Compliance Files

Feluda can generate essential compliance files required for commercial software distribution and open source projects.

### NOTICE File

A **NOTICE file** is a concise summary document that provides attribution for third-party components:

- **Purpose**: Quick overview of all third-party components and their licenses
- **Content**: Organized by license type, lists all dependencies with their versions
- **Use Cases**: 
  - Legal compliance documentation
  - Quick reference for license audits
  - Attribution requirements for many open source licenses

### THIRD_PARTY_LICENSES File

A **THIRD_PARTY_LICENSES file** provides comprehensive license documentation:

- **Purpose**: Complete legal documentation for all dependencies
- **Content**: Full license texts, compatibility analysis, package URLs, and copyright information
- **Use Cases**:
  - Commercial software distribution requirements
  - Legal compliance for enterprise applications
  - Due diligence for acquisitions and audits
  - App store submissions (iOS, Android, etc.)

### Why These Files Are Important

**Legal Protection**: Many open source licenses require attribution when redistributing code. These files ensure compliance and protect your organization from legal issues.

**Transparency**: Shows exactly what third-party code is included in your application, building trust with users and stakeholders.

**Commercial Readiness**: Essential for commercial software, enterprise deployments, and app store submissions.

**Audit Preparation**: Makes license audits faster and easier by providing all necessary documentation in standard formats.

### Important Legal Notice

**⚠️ DISCLAIMER**: Feluda is still in early stages. While we're trying to follow through all compliances, users are responsible for:

- **Verifying accuracy** of all license information
- **Ensuring compliance** with all applicable license terms
- **Consulting legal counsel** for license compliance matters
- **Checking official repositories** for up-to-date license information

Feluda and its contributors disclaim all warranties and are not liable for any legal issues arising from the use of this information. **Use at your own risk.**

### When You Need These Files

- 📱 **Mobile app distribution** (iOS App Store, Google Play)
- 🏢 **Enterprise software deployment**
- 💼 **Commercial product releases**
- 🔍 **Legal compliance audits**
- 🤝 **Open source project attribution**
- 📄 **Regulatory compliance** (GDPR, SOX, etc.)

## Output Format

### JSON

- Default: Plain text.
- JSON: Use the `--json` flag for JSON output.

```sh
feluda --json
```

Sample Output for a sample cargo.toml file containing `serde` and `tokio` dependencies:

```json
[
  {
    "name": "serde",
    "version": "1.0.151",
    "license": "MIT",
    "is_restrictive": false,
    "compatibility": "Compatible"
  },
  {
    "name": "tokio",
    "version": "1.0.2",
    "license": "MIT",
    "is_restrictive": false,
    "compatibility": "Compatible"
  }
]
```

### YAML

Use the `--yaml` flag for YAML output

```sh
feluda --yaml
```

Sample Output for a sample cargo.toml file containing `serde` and `tokio` dependencies:

```yaml
- name: serde
  version: 1.0.151
  license: MIT
  is_restrictive: false
  compatibility: Compatible
- name: tokio
  version: 1.0.2
  license: MIT
  is_restrictive: false
  compatibility: Compatible
```

### Gist Mode

For a short summary, in case you don't want all that output covering your screen:

```sh
feluda --gist
```

<img width="610" height="257" alt="feluda-gist" src="https://github.com/user-attachments/assets/51224a92-678d-4cd6-8a18-45a4e67f97f2" />

### Verbose Mode

For detailed information about each dependency:

```sh
feluda --verbose
```

### License Compatibility

Feluda can check if dependency licenses are compatible with your project's license:

```sh
feluda --project-license MIT
```

You can also filter for incompatible licenses only:

```sh
feluda --incompatible
```

And fail CI builds if incompatible licenses are found:

```sh
feluda --fail-on-incompatible
```

### Strict Mode

In case you strictly need only the restrictive dependencies:

```sh
feluda --strict
```

### Terminal User Interface (TUI) Mode

We've an awesome ✨ TUI mode available to browse through the dependencies in a visually appealing way as well:

```sh
feluda --gui
```

![ss-gui](https://github.com/user-attachments/assets/44d46755-b186-4326-a3fb-548da31f3acd)

## CI/CD Integration

Feluda provides several options for CI integration:

- `--ci-format <github|jenkins>`: Generate output compatible with the specified CI system
- `--fail-on-restrictive`: Make the CI build fail when restrictive licenses are found
- `--fail-on-incompatible`: Make the CI build fail when incompatible licenses are found
- `--output-file <path>`: Write the output to a file instead of stdout

Feluda can be easily integrated into your CI/CD pipelines with built-in support for **GitHub Actions** and **Jenkins**.

### GitHub Actions

To use Feluda with GitHub Actions, create a `.github/workflows/feluda.yml` file with the following content:

```yaml
name: License Check

on:
  push:
    branches: [ main ]
  pull_request:
    branches: [ main ]

jobs:
  check-licenses:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v3

      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          profile: minimal
          toolchain: stable
          override: true

      - name: Install Feluda
        run: cargo install feluda

      - name: Check licenses
        run: feluda --ci-format github --fail-on-restrictive --fail-on-incompatible

      - name: Generate compliance files
        run: |
          echo "1" | feluda generate  # Auto-select NOTICE file
          echo "2" | feluda generate  # Auto-select THIRD_PARTY_LICENSES file

      - name: Upload compliance artifacts
        uses: actions/upload-artifact@v3
        with:
          name: license-compliance
          path: |
            NOTICE
            THIRD_PARTY_LICENSES.md
```

Checkout [contributing guidelines](./CONTRIBUTING.md) if you are looking to contribute to this project.

> Currently, using [choosealicense](https://choosealicense.com/) license directory for source of truth.

## Configuration (Optional)

Feluda allows you to customize which licenses are considered restrictive through configuration. This can be done in three ways, listed in order of precedence (highest to lowest):

1. Environment variables
2. `.feluda.toml` configuration file
3. Default values

### Default Restrictive Licenses

By default, Feluda considers the following licenses as restrictive:
- GPL-3.0
- AGPL-3.0
- LGPL-3.0
- MPL-2.0
- SEE LICENSE IN LICENSE
- CC-BY-SA-4.0
- EPL-2.0

### Configuration File

Create a `.feluda.toml` file in your project root to override the default restrictive licenses:

```toml
[licenses]
# Override the default list of restrictive licenses
restrictive = [
    "GPL-3.0",      # GNU General Public License v3.0
    "AGPL-3.0",     # GNU Affero General Public License v3.0
    "Custom-1.0",   # Your custom license identifier
]
```

### Environment Variables

You can also override the configuration using environment variables:

```sh
# Override restrictive licenses list
export FELUDA_LICENSES_RESTRICTIVE='["GPL-3.0","AGPL-3.0","Custom-1.0"]'
```

The environment variables take precedence over both the configuration file and default values.

---

## License

Feluda is licensed under the [MIT License](./LICENSE).

![felu](https://github.com/user-attachments/assets/5f2bf6c4-3b70-4d2f-9990-c4005f56c5a9)

_Happy coding with Feluda!_ 🚀
