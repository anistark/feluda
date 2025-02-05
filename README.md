# Feluda

[![Crates.io Version](https://img.shields.io/crates/v/feluda)
](https://crates.io/crates/feluda) ![maintenance-status](https://img.shields.io/badge/maintenance-actively--developed-brightgreen.svg)

![Crates.io Downloads (latest version)](https://img.shields.io/crates/dv/feluda)

🔎 **Feluda** is a Rust-based command-line tool that analyzes the dependencies of a project, notes down their licenses, and flags any permissions that restrict personal or commercial usage.

![ss-1](https://github.com/user-attachments/assets/bda9fb10-3e6b-4881-b852-58ab33341dcd)

> 👋 It's still highly experimental, but fast iterating. Welcoming contributors and support to help bring out this project even better!

## Features

- Parse your project to identify dependencies and their licenses.
- Classify licenses into permissive, restrictive, or unknown categories.
- Flag dependencies with licenses that may restrict personal or commercial use.
- Output results in plain text, JSON or TUI formats. There's also a gist format which is available in strict mode to output a single line only.

### Support Languages

- [x] [Rust](https://www.rust-lang.org/)
- [x] [NodeJs](https://nodejs.org/)
- [x] [Go](https://go.dev/)
- [ ] [Python](https://www.python.org/)

_If your fav language or framework isn't supported, feel free to open an feature request issue! 👋_

## Installation

### Official Distribution 🎉:

<details>
<summary>Rust</summary>
  

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
<summary>Arch Linux (maintained by @adamperkowski)</summary>

[feluda](https://aur.archlinux.org/packages/feluda) is available in the [AUR](https://aur.archlinux.org/).
You can install it using an AUR helper (e.g. paru):

```sh
paru -S feluda
```

</details>

<details>
<summary>NetBSD (maintained by @0323pin)</summary>

On NetBSD a package is available from the [official repositories](https://pkgsrc.se/devel/feluda/). To install it, simply run:

```sh
pkgin install feluda
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
    "is_restrictive": false
  },
  {
    "name": "tokio",
    "version": "1.0.2",
    "license": "MIT",
    "is_restrictive": false
  }
]
```

### Verbose Mode

For detailed information about each dependency:

```sh
feluda --verbose
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

![ss-gui](https://github.com/user-attachments/assets/67170931-7fde-4bb0-b4b0-8640b8a261d6)

Checkout [contributing guidelines](./CONTRIBUTING.md) if you are looking to contribute to this project.

> Currently, using [choosealicense](https://choosealicense.com/) license directory for source of truth.

---

## License

Feluda is licensed under the [MIT License](./LICENSE).

_Happy coding with Feluda!_ 🚀
