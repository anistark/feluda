//! An external command that never finishes is stopped at its deadline, and the scan carries on.
//!
//! Every command feluda runs goes through `src/command.rs`. The fixture is a Go project with a
//! fake `go` first on `PATH` that hangs for any subcommand, so both `go mod graph` (a listing
//! command, `[timeouts] command`) and `go env GOMODCACHE` (a lookup, `[timeouts] request`) run past
//! their deadline, set to one second each here. The scan has to finish well inside the fake's
//! sleep, fall back to the direct dependency in `go.mod`, resolve its license from the module
//! cache under `GOPATH`, and say on stderr, once, that `go mod graph` was stopped.
//!
//! The fake is a shell script, so the process feluda kills is `sh` and the `sleep` it started keeps
//! the pipes open: this is also the grandchild case, which must not hold the scan either.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

const MIT_TEXT: &str = "MIT License\n\nCopyright (c) 2026 Fixture\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files\n";

#[test]
fn a_hanging_go_is_stopped_and_the_scan_falls_back_to_go_mod() {
    let temp = tempfile::tempdir().expect("failed to create temp dir");
    let root = temp.path();

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("LICENSE"), MIT_TEXT).unwrap();
    fs::write(
        project.join("go.mod"),
        "module example.com/fixture\n\ngo 1.22\n\nrequire example.com/mylib v1.2.3\n",
    )
    .unwrap();

    let gopath = root.join("gopath");
    let module_dir = gopath.join("pkg/mod/example.com/mylib@v1.2.3");
    fs::create_dir_all(&module_dir).unwrap();
    fs::write(module_dir.join("LICENSE"), MIT_TEXT).unwrap();

    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fake_go = bin.join("go");
    fs::write(&fake_go, "#!/bin/sh\nsleep 60\n").unwrap();
    fs::set_permissions(&fake_go, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_feluda"))
        .args(["--path", project.to_str().unwrap(), "--json"])
        .current_dir(root)
        .env("PATH", path)
        .env("HOME", root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("GOPATH", &gopath)
        .env_remove("GOMODCACHE")
        .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
        .env("FELUDA_TIMEOUTS_COMMAND", "1")
        .env("FELUDA_TIMEOUTS_REQUEST", "1")
        .output()
        .expect("failed to run feluda binary");
    let elapsed = started.elapsed();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "feluda exited with {:?}\nstderr: {stderr}",
        output.status.code()
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "the scan waited on the hanging command: {elapsed:?}"
    );
    assert_eq!(
        stderr
            .matches("`go mod graph` did not finish within 1 s")
            .count(),
        1,
        "stderr: {stderr}"
    );

    let report: Vec<Value> =
        serde_json::from_slice(&output.stdout).expect("feluda emitted invalid JSON");
    let dep = report
        .iter()
        .find(|entry| entry["name"] == "example.com/mylib")
        .unwrap_or_else(|| panic!("no example.com/mylib in report: {report:#?}"));
    assert_eq!(dep["license"], "MIT");
    assert_eq!(dep["version"], "v1.2.3");
}
