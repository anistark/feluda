# Feluda

[![Crates.io Version](https://img.shields.io/crates/v/feluda)
](https://crates.io/crates/feluda) [![Crates.io Downloads](https://img.shields.io/crates/d/feluda)](https://crates.io/crates/feluda) [![Crates.io Downloads (latest version)](https://img.shields.io/crates/dv/feluda)](https://crates.io/crates/feluda) [![Open Source](https://img.shields.io/badge/open-source-brightgreen)](https://github.com/anistark/feluda) [![Contributors](https://img.shields.io/github/contributors/anistark/feluda)](https://github.com/anistark/feluda/graphs/contributors) ![maintenance-status](https://img.shields.io/badge/maintenance-actively--developed-brightgreen.svg)

🔎 **Feluda** is a Rust-based command-line tool that analyzes the dependencies of a project, notes down their licenses, and flags any permissions that restrict personal or commercial usage or are incompatible with your project's license.

![ss](https://github.com/user-attachments/assets/473908eb-43cb-4c4f-86aa-017de251afa8)

> 👋 It's still highly experimental, but fast iterating. Welcoming contributors and support to help bring out this project even better!

## Features

- Parse your project to identify dependencies and their licenses.
- Classify licenses into permissive, restrictive, or unknown categories.
- Check license compatibility between dependencies and your project's license.
- Flag dependencies with licenses that may restrict personal or commercial use.
- Flag dependencies with licenses that may be incompatible with your project's license.
- Output results in plain text, JSON or TUI formats. There's also a gist format which is available in strict mode to output a single line only.
- CI/CD support for Github Actions and Jenkins.
- Verbose mode gives an enhanced view of all licenses.

### Support Languages

1. ![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
2. ![TypeScript](https://img.shields.io/badge/typescript-%23007ACC.svg?style=for-the-badge&logo=typescript&logoColor=white) ![JavaScript](https://img.shields.io/badge/javascript-%23323330.svg?style=for-the-badge&logo=javascript&logoColor=%23F7DF1E) ![NodeJS](https://img.shields.io/badge/node.js-6DA55F?style=for-the-badge&logo=node.js&logoColor=white)
3. ![Go](https://img.shields.io/badge/go-%2300ADD8.svg?style=for-the-badge&logo=go&logoColor=white)
4. ![Python](https://img.shields.io/badge/python-3670A0?style=for-the-badge&logo=python&logoColor=ffdd54)

Feluda supports analyzing dependencies across multiple languages simultaneously. You can also filter the analysis to a specific language using the `--language` flag.


```sh
feluda --language {rust|node|go|python}
```

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

### Basic Usage

Run the tool in the project directory:

```sh
feluda
```

### Specify a Path to your project directory

```sh
feluda --path /path/to/project/
```

_If you're using Feluda, feel free to grab a Scanned with Feluda badge for your project:_ [![Scanned with Feluda](https://img.shields.io/badge/Scanned%20with-Feluda-red)](https://github.com/anistark/feluda)
```
[![Scanned with Feluda](https://img.shields.io/badge/Scanned%20with-Feluda-red)](https://github.com/anistark/feluda)
```

### Output Format

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

- YAML: Use the `--yaml` flag for YAML output.

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

### TUI Mode

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
```

Checkout [contributing guidelines](./CONTRIBUTING.md) if you are looking to contribute to this project.

> Currently, using [choosealicense](https://choosealicense.com/) license directory for source of truth.

## Configuration

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
