---
name: feluda
description: >
  License compliance check with feluda. Proactively invoke this skill whenever:
  (1) a dependency manifest is added or modified in the diff — Cargo.toml,
  package.json, go.mod, requirements.txt, pyproject.toml, pom.xml, build.gradle,
  Gemfile, composer.json, or similar; (2) the user is about to commit or push and
  dependency files appear in the staged changes; (3) the user asks to "check
  licenses", "audit dependencies", "is X license safe to use", or "will this dep
  cause legal issues"; (4) the user adds a new package via cargo add, npm install,
  pip install, go get, etc. Always run this before confirming a commit that
  introduces new third-party dependencies.
allowed-tools: Bash Glob Grep Read
---

# Feluda: License Compliance Skill

[Feluda](https://github.com/anistark/feluda) is a fast, local dependency license
scanner that flags GPL, AGPL, and other restrictive or incompatible licenses before
they land in a commit. This skill runs it automatically and surfaces results inline.

---

## Step 1 — Verify feluda is installed

```bash
feluda --version 2>/dev/null || echo "FELUDA_NOT_INSTALLED"
```

If the output contains `FELUDA_NOT_INSTALLED`, stop and tell the user:

> **feluda is not installed.** Install it with one of:
>
> ```sh
> cargo install feluda          # via Rust (any OS)
> brew install feluda            # via Homebrew (macOS/Linux)
> ```
>
> Or download a binary from https://github.com/anistark/feluda/releases
>
> Once installed, re-run this check.

---

## Step 2 — Identify changed dependency files

Run in parallel to see what changed:

```bash
git diff --cached --name-only 2>/dev/null
git diff HEAD --name-only 2>/dev/null
```

Trigger a full feluda scan if any of these appear:

| Ecosystem | Files |
|-----------|-------|
| Rust | `Cargo.toml`, `Cargo.lock` |
| Node.js | `package.json`, `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lockb` |
| Go | `go.mod`, `go.sum` |
| Python | `requirements.txt`, `pyproject.toml`, `Pipfile.lock`, `pip_freeze.txt` |
| Java | `pom.xml`, `build.gradle`, `build.gradle.kts` |
| .NET | `*.csproj`, `*.fsproj`, `*.vbproj`, `*.slnx` |
| Ruby | `Gemfile`, `Gemfile.lock` |
| PHP | `composer.json`, `composer.lock` |
| C/C++ | `vcpkg.json`, `conanfile.txt`, `CMakeLists.txt` |
| R | `DESCRIPTION`, `renv.lock` |

If the user explicitly asked for a license check, skip this step and go straight to
Step 3 regardless of the diff.

If no dependency files appear in the diff and the user has not explicitly asked, say:

> No dependency changes detected in the current diff. Skipping feluda scan.
> Run `feluda` manually for a full project scan.

---

## Step 3 — Run feluda

Run from the project root. Start with the focused scan:

```bash
feluda --restrictive 2>&1; echo "FELUDA_EXIT:$?"
```

`--restrictive` filters to only licenses that are likely to cause issues (GPL, AGPL,
SSPL, BUSL, CC-BY-NC, etc.) — the most actionable output for a pre-commit check.

For a full scan of all dependencies and licenses (when the user asks for a complete
audit), run:

```bash
feluda --verbose 2>&1
```

If the project has a `.feluda.toml`, feluda reads it automatically — no extra
flags needed for configured restrictions or ignores.

For monorepos or specific subdirectories:

```bash
feluda --path ./packages/my-lib --restrictive 2>&1; echo "FELUDA_EXIT:$?"
```

---

## Step 4 — Interpret and surface the results

Parse the exit code from `FELUDA_EXIT:N`.

### Exit 0 — all clear

> ✓ feluda: no restrictive licenses found.

If the output mentions dependencies with **unknown** licenses, add:

> Note: some dependencies have unresolved licenses. Set `GITHUB_TOKEN` in your
> environment for higher API rate limits, which helps feluda resolve more unknowns.

### Exit 1 — restrictive or incompatible licenses found

Extract the flagged packages from the output and present them clearly:

> ⚠ feluda found license issues in the current dependencies:
>
> | Package | License | Why it matters |
> |---------|---------|----------------|
> | `some-lib@1.2.0` | GPL-3.0 | Copyleft — any distribution requires source disclosure |
> | `other-lib@3.0.0` | AGPL-3.0 | Network copyleft — source disclosure required even for SaaS |
>
> **Options before committing:**
> 1. Find a MIT/Apache-2.0 alternative for the flagged packages.
> 2. If the dep is intentional, add it to `.feluda.toml`:
>    ```toml
>    [licenses]
>    ignore = ["GPL-3.0"]
>    ```
> 3. Run `feluda --verbose` for full compatibility details and OSI status.

---

## Step 5 — Pre-commit hook guidance

If this is the first time a license issue has been found in this project, suggest
setting up automatic enforcement:

> To catch license issues automatically before every commit, run:
>
> ```sh
> feluda init
> ```
>
> This creates `.feluda.toml` (project config) and adds a `feluda` hook to
> `.pre-commit-config.yaml`. After that, `git commit` will refuse if restrictive
> licenses are present.

---

## Quick reference

```sh
feluda                           # Full scan, all licenses
feluda --restrictive             # Only restrictive licenses (pre-commit focus)
feluda --incompatible            # Only licenses incompatible with your project
feluda --project-license MIT     # Check compatibility against a specific license
feluda --fail-on-restrictive     # Exit 1 if any restrictive found (CI gate)
feluda --json                    # Machine-readable JSON output
feluda --verbose                 # OSI status, compatibility matrix, full details
feluda --path ./sub/dir          # Scan a specific subdirectory
feluda generate                  # Generate NOTICE / THIRD_PARTY_LICENSES file
feluda sbom                      # Generate SPDX or CycloneDX SBOM
feluda init                      # Set up .feluda.toml + pre-commit hook
feluda cache --clear             # Clear the GitHub license data cache
```
