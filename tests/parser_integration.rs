//! Integration tests across the language parsers (#120).
//!
//! Each test builds a self-contained fixture project in a temp directory and drives the real
//! `feluda` binary (`CARGO_BIN_EXE_feluda`) against it, asserting on the `--json` report. The
//! fixtures are crafted so license resolution succeeds from local sources alone — Node from
//! `node_modules/*/package.json`, Rust from `cargo metadata` on a path dependency, Go from a
//! module cache directory pointed at by `GOMODCACHE`/`GOPATH` — so the tests hold with or
//! without network access.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

/// MIT license body that `detect_license_from_content` recognises; used both as fixture
/// project licenses and as module-cache license files.
const MIT_TEXT: &str = "MIT License\n\nCopyright (c) 2026 Fixture\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files\n";

fn run_feluda(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_feluda"));
    cmd.current_dir(dir)
        .arg("--path")
        .arg(dir.to_str().unwrap())
        .args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    cmd.output().expect("failed to run feluda binary")
}

/// Run `feluda --json` on `dir` and parse the report. An empty report (no dependencies at
/// all) produces no stdout, which parses as an empty list.
fn scan_json(dir: &Path, extra_args: &[&str], envs: &[(&str, &str)]) -> Vec<Value> {
    let mut args = vec!["--json"];
    args.extend_from_slice(extra_args);
    let output = run_feluda(dir, &args, envs);
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    serde_json::from_str(trimmed)
        .unwrap_or_else(|e| panic!("feluda emitted invalid JSON: {e}\n{stdout}"))
}

fn entry<'a>(entries: &'a [Value], name: &str) -> &'a Value {
    entries
        .iter()
        .find(|e| e["name"] == name)
        .unwrap_or_else(|| panic!("no entry named {name:?} in report: {entries:#?}"))
}

fn has_entry(entries: &[Value], name: &str) -> bool {
    entries.iter().any(|e| e["name"] == name)
}

/// Write a Node fixture: root `package.json` plus a populated `node_modules` so licenses
/// resolve locally instead of via the npm registry.
fn write_node_fixture(root: &Path, deps: &[(&str, &str, &str)]) {
    let dep_list = deps
        .iter()
        .map(|(name, version, _)| format!("    \"{name}\": \"{version}\""))
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        root.join("package.json"),
        format!(
            "{{\n  \"name\": \"feluda-integration-fixture\",\n  \"version\": \"1.0.0\",\n  \"license\": \"MIT\",\n  \"dependencies\": {{\n{dep_list}\n  }}\n}}\n"
        ),
    )
    .unwrap();

    for (name, version, license) in deps {
        let pkg_dir = root.join("node_modules").join(name);
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(
            pkg_dir.join("package.json"),
            format!(
                "{{\n  \"name\": \"{name}\",\n  \"version\": \"{version}\",\n  \"license\": \"{license}\"\n}}\n"
            ),
        )
        .unwrap();
    }
}

/// Write a Go fixture: `go.mod` requiring `module` plus a fake module cache holding its
/// license file. Returns the env vars that point feluda's resolution at the fake cache
/// (`GOMODCACHE` when the `go` binary answers `go env`, `GOPATH` for the fallback path).
fn write_go_fixture(root: &Path, module: &str, version: &str) -> Vec<(String, String)> {
    fs::write(
        root.join("go.mod"),
        format!("module example.com/fixture\n\ngo 1.22\n\nrequire (\n\t{module} {version}\n)\n"),
    )
    .unwrap();

    let gopath = root.join("gopath");
    let cache = gopath.join("pkg").join("mod");
    let module_dir = cache.join(format!("{module}@{version}"));
    fs::create_dir_all(&module_dir).unwrap();
    fs::write(module_dir.join("LICENSE"), MIT_TEXT).unwrap();

    vec![
        ("GOPATH".to_string(), gopath.display().to_string()),
        ("GOMODCACHE".to_string(), cache.display().to_string()),
    ]
}

fn as_env(envs: &[(String, String)]) -> Vec<(&str, &str)> {
    envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()
}

#[test]
fn node_dependencies_resolve_from_local_node_modules() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("LICENSE"), MIT_TEXT).unwrap();
    write_node_fixture(
        root,
        &[
            ("fixture-permissive", "1.3.0", "ISC"),
            ("fixture-copyleft", "2.0.0", "GPL-3.0-only"),
        ],
    );

    let entries = scan_json(root, &[], &[]);

    let permissive = entry(&entries, "fixture-permissive");
    assert_eq!(permissive["license"], "ISC");
    assert_eq!(permissive["version"], "1.3.0");
    assert_eq!(permissive["is_restrictive"], false);

    let copyleft = entry(&entries, "fixture-copyleft");
    assert_eq!(copyleft["license"], "GPL-3.0-only");
    assert_eq!(copyleft["is_restrictive"], true);
    assert_eq!(copyleft["compatibility"], "Incompatible");
}

#[test]
fn fail_on_restrictive_sets_exit_code() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("LICENSE"), MIT_TEXT).unwrap();
    write_node_fixture(root, &[("fixture-copyleft", "2.0.0", "AGPL-3.0")]);

    let failing = run_feluda(root, &["--json", "--fail-on-restrictive"], &[]);
    assert_eq!(
        failing.status.code(),
        Some(1),
        "restrictive dependency must fail the scan\nstderr: {}",
        String::from_utf8_lossy(&failing.stderr)
    );

    let passing = run_feluda(root, &["--json"], &[]);
    assert!(
        passing.status.success(),
        "without --fail-on-restrictive the scan must exit 0"
    );
}

#[test]
fn rust_path_dependency_license_from_cargo_metadata() {
    let temp = tempfile::TempDir::new().unwrap();
    let app = temp.path().join("app");
    let lib = temp.path().join("locallib");
    fs::create_dir_all(app.join("src")).unwrap();
    fs::create_dir_all(lib.join("src")).unwrap();

    fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"MIT\"\n\n[dependencies]\nlocallib = { path = \"../locallib\" }\n",
    )
    .unwrap();
    fs::write(app.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        lib.join("Cargo.toml"),
        "[package]\nname = \"locallib\"\nversion = \"0.3.1\"\nedition = \"2021\"\nlicense = \"MPL-2.0\"\n",
    )
    .unwrap();
    fs::write(lib.join("src/lib.rs"), "").unwrap();

    let entries = scan_json(&app, &[], &[]);

    let dep = entry(&entries, "locallib");
    assert_eq!(dep["license"], "MPL-2.0");
    assert_eq!(dep["version"], "0.3.1");
    // MPL-2.0 is in the default restrictive config and carries `disclose-source` in the
    // GitHub registry, so both classification paths agree.
    assert_eq!(dep["is_restrictive"], true);
}

#[test]
fn go_dependency_license_from_module_cache() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("LICENSE"), MIT_TEXT).unwrap();
    let envs = write_go_fixture(root, "example.com/mylib", "v1.2.3");

    let entries = scan_json(root, &[], &as_env(&envs));

    let dep = entry(&entries, "example.com/mylib");
    assert_eq!(dep["license"], "MIT");
    assert_eq!(dep["version"], "v1.2.3");
    assert_eq!(dep["is_restrictive"], false);
    assert_eq!(dep["compatibility"], "Compatible");
}

#[test]
fn python_requirements_without_dependencies_scans_clean() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("LICENSE"), MIT_TEXT).unwrap();
    fs::write(root.join("requirements.txt"), "# no dependencies\n").unwrap();

    let entries = scan_json(root, &[], &[]);
    assert!(
        entries.is_empty(),
        "empty requirements.txt must produce an empty report: {entries:#?}"
    );
}

#[test]
fn own_source_header_findings_reported() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("LICENSE"), MIT_TEXT).unwrap();
    write_node_fixture(root, &[]);

    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    // Pasted file with a foreign SPDX tag: must be flagged.
    fs::write(
        src.join("pasted.py"),
        "# SPDX-License-Identifier: GPL-3.0-only\ndef pasted():\n    pass\n",
    )
    .unwrap();
    // GNU grant banner without an SPDX tag: must also be flagged.
    fs::write(
        src.join("borrowed.c"),
        "// This program is free software; you can redistribute it and/or modify\n// it under the terms of the GNU General Public License as published by\n// the Free Software Foundation; either version 2 of the License, or\n// (at your option) any later version.\nint main(void) { return 0; }\n",
    )
    .unwrap();
    // Own header matching the project license: must not be flagged.
    fs::write(
        src.join("own.ts"),
        "// SPDX-License-Identifier: MIT\nexport const ok = true;\n",
    )
    .unwrap();

    let entries = scan_json(root, &[], &[]);

    let pasted = entry(&entries, "src/pasted.py");
    assert_eq!(pasted["version"], "own source");
    assert_eq!(pasted["license"], "GPL-3.0-only");
    assert_eq!(pasted["is_restrictive"], true);
    assert_eq!(pasted["compatibility"], "Incompatible");

    let borrowed = entry(&entries, "src/borrowed.c");
    assert_eq!(borrowed["license"], "GPL-2.0-or-later");
    assert_eq!(borrowed["is_restrictive"], true);

    assert!(
        !has_entry(&entries, "src/own.ts"),
        "file whose header matches the project license must not be flagged: {entries:#?}"
    );
}

#[test]
fn multi_language_root_parses_all_ecosystems() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("LICENSE"), MIT_TEXT).unwrap();
    write_node_fixture(root, &[("fixture-permissive", "1.3.0", "ISC")]);
    let envs = write_go_fixture(root, "example.com/mylib", "v1.2.3");
    fs::write(root.join("requirements.txt"), "# no dependencies\n").unwrap();

    let entries = scan_json(root, &[], &as_env(&envs));

    assert!(has_entry(&entries, "fixture-permissive"));
    assert!(has_entry(&entries, "example.com/mylib"));
}

#[test]
fn language_filter_limits_scan() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path();
    fs::write(root.join("LICENSE"), MIT_TEXT).unwrap();
    write_node_fixture(root, &[("fixture-permissive", "1.3.0", "ISC")]);
    let envs = write_go_fixture(root, "example.com/mylib", "v1.2.3");

    let go_only = scan_json(root, &["--language", "go"], &as_env(&envs));
    assert!(has_entry(&go_only, "example.com/mylib"));
    assert!(
        !has_entry(&go_only, "fixture-permissive"),
        "--language go must exclude Node dependencies: {go_only:#?}"
    );

    let node_only = scan_json(root, &["--language", "node"], &as_env(&envs));
    assert!(has_entry(&node_only, "fixture-permissive"));
    assert!(
        !has_entry(&node_only, "example.com/mylib"),
        "--language node must exclude Go dependencies: {node_only:#?}"
    );
}
