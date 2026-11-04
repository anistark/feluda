//! `feluda update` and `feluda upgrade` against a stub release server and fake package managers.
//!
//! Install detection reads the path the binary runs from, so each test copies the real binary to
//! where that install method puts it. `PATH` holds only the fake tools, which append their
//! arguments to a log, so nothing on the machine running the tests is touched.

#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use semver::Version;
use tempfile::TempDir;

fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("package version is not semver")
}

fn next_patch() -> String {
    let version = current();
    format!("{}.{}.{}", version.major, version.minor, version.patch + 1)
}

fn next_minor() -> String {
    let version = current();
    format!("{}.{}.0", version.major, version.minor + 1)
}

fn next_major() -> String {
    format!("{}.0.0", current().major + 1)
}

fn index_of(rows: &[(&str, bool)]) -> String {
    rows.iter()
        .map(|(version, yanked)| {
            format!("{{\"name\":\"feluda\",\"vers\":\"{version}\",\"yanked\":{yanked}}}\n")
        })
        .collect()
}

/// A GitHub releases listing, each release carrying a `.deb` for this architecture, served from
/// `base`, unless it is named in `without_package`.
fn releases_of(base: &str, versions: &[&str], without_package: &[&str]) -> String {
    let arch = std::env::consts::ARCH;
    let rows: Vec<String> = versions
        .iter()
        .map(|version| {
            let assets = if without_package.contains(version) {
                String::new()
            } else {
                let name = format!("feluda-v{version}-{arch}.deb");
                format!(r#"{{"name":"{name}","browser_download_url":"{base}/download/{name}"}}"#)
            };
            format!(r#"{{"tag_name":"v{version}","assets":[{assets}]}}"#)
        })
        .collect();
    format!("[{}]", rows.join(","))
}

/// Answers `/index` and `/releases` with `routes`, and `/download/*` with a stand-in package.
fn serve(routes: impl Fn(&str) -> String + Send + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind");
    let port = listener.local_addr().expect("no address").port();
    let base = format!("http://127.0.0.1:{port}");
    let own_base = base.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let read = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..read]);
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            let body = if path.starts_with("/download/") {
                "package bytes".to_string()
            } else {
                routes(&format!("{own_base}{path}"))
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    base
}

/// A base URL that answers every request the way GitHub does once an address runs out of quota.
fn rate_limited() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind");
    let port = listener.local_addr().expect("no address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    format!("http://127.0.0.1:{port}")
}

struct Install {
    root: TempDir,
    exe: PathBuf,
    tools: PathBuf,
    log: PathBuf,
}

impl Install {
    /// The real binary, copied to `relative_path` under a fresh root.
    fn at(relative_path: &str) -> Self {
        let root = tempfile::tempdir().expect("failed to create temp root");
        let exe = root.path().join(relative_path);
        fs::create_dir_all(exe.parent().expect("no parent")).expect("failed to create bin dir");
        fs::copy(env!("CARGO_BIN_EXE_feluda"), &exe).expect("failed to copy feluda");
        let tools = root.path().join("tools");
        fs::create_dir_all(&tools).expect("failed to create tools dir");
        let log = root.path().join("tools.log");
        fs::write(&log, "").expect("failed to create log");
        Self {
            root,
            exe,
            tools,
            log,
        }
    }

    /// A tool that logs how it was called and then runs `body`.
    fn tool(&self, name: &str, body: &str) -> &Self {
        let path = self.tools.join(name);
        let log = self.log.display();
        fs::write(
            &path,
            format!("#!/bin/sh\necho \"{name} $*\" >> \"{log}\"\n{body}\n"),
        )
        .expect("failed to write fake tool");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("failed to mark fake tool executable");
        self
    }

    /// What `feluda --version` prints once the install has finished.
    fn reports_version(&self, version: &str) -> &Self {
        let path = self.tools.join("feluda");
        fs::write(&path, format!("#!/bin/sh\necho 'Feluda v{version}'\n"))
            .expect("failed to write fake feluda");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("failed to mark fake feluda executable");
        self
    }

    fn run(&self, base: &str, args: &[&str]) -> Output {
        Command::new(&self.exe)
            .args(args)
            .current_dir(self.root.path())
            .env_clear()
            .env("PATH", &self.tools)
            .env("HOME", self.root.path())
            .env("XDG_CACHE_HOME", self.root.path().join("cache"))
            .env("FELUDA_CLEARLYDEFINED_ENABLED", "false")
            .env("FELUDA_UPDATE_INDEX", format!("{base}/index"))
            .env("FELUDA_UPDATE_RELEASES", format!("{base}/releases"))
            .output()
            .expect("failed to run feluda")
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .expect("no log")
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn cargo_install() -> Install {
    let install = Install::at(".cargo/bin/feluda");
    install.tool("cargo", "");
    install
}

fn crate_index(rows: Vec<(String, bool)>) -> String {
    serve(move |url| {
        assert!(url.ends_with("/index"), "asked the wrong source: {url}");
        let rows: Vec<(&str, bool)> = rows.iter().map(|(v, y)| (v.as_str(), *y)).collect();
        index_of(&rows)
    })
}

fn github_releases(versions: Vec<String>, without_package: Vec<String>) -> String {
    serve(move |url| {
        assert!(url.ends_with("/releases"), "asked the wrong source: {url}");
        let base = url.trim_end_matches("/releases");
        let versions: Vec<&str> = versions.iter().map(String::as_str).collect();
        let without: Vec<&str> = without_package.iter().map(String::as_str).collect();
        releases_of(base, &versions, &without)
    })
}

#[test]
fn update_installs_the_newest_release_of_the_current_major() {
    let install = cargo_install();
    install.reports_version(&next_minor());
    let base = crate_index(vec![
        (next_patch(), false),
        (next_minor(), false),
        (
            format!("{}.{}.0", current().major, current().minor + 2),
            true,
        ),
        (format!("{}-rc.1", next_major()), false),
        (next_major(), false),
    ]);

    let output = install.run(&base, &["update"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        install.calls(),
        vec![format!(
            "cargo install feluda --version {} --locked",
            next_minor()
        )]
    );
    let text = stdout(&output);
    assert!(
        text.contains(&format!("Feluda is now v{}", next_minor())),
        "{text}"
    );
    assert!(text.contains("feluda upgrade"), "{text}");
}

#[test]
fn upgrade_crosses_a_major_version() {
    let install = cargo_install();
    install.reports_version(&next_major());
    let base = crate_index(vec![(next_minor(), false), (next_major(), false)]);

    let output = install.run(&base, &["upgrade"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        install.calls(),
        vec![format!(
            "cargo install feluda --version {} --locked",
            next_major()
        )]
    );
}

#[test]
fn update_leaves_a_new_major_to_upgrade() {
    let install = cargo_install();
    let base = crate_index(vec![(next_major(), false)]);

    let output = install.run(&base, &["update"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(install.calls(), Vec::<String>::new());
    assert!(stdout(&output).contains("feluda upgrade"));
}

#[test]
fn check_says_what_would_happen_and_does_nothing() {
    let install = cargo_install();
    let base = crate_index(vec![(next_minor(), false)]);

    let output = install.run(&base, &["update", "--check"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(install.calls(), Vec::<String>::new());
    let text = stdout(&output);
    assert!(text.contains("installed with cargo"), "{text}");
    assert!(text.contains("cargo install feluda --version"), "{text}");
}

#[test]
fn nothing_newer_runs_nothing() {
    let install = cargo_install();
    let base = crate_index(vec![(current().to_string(), false)]);

    for command in ["update", "upgrade"] {
        let output = install.run(&base, &[command]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(stdout(&output).contains("latest version"));
    }
    assert_eq!(install.calls(), Vec::<String>::new());
}

#[test]
fn a_failed_install_fails_the_command() {
    let install = Install::at(".cargo/bin/feluda");
    install.tool("cargo", "exit 3");
    let base = crate_index(vec![(next_minor(), false)]);

    let output = install.run(&base, &["update"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("failed"), "{}", stderr(&output));
}

#[test]
fn homebrew_runs_brew_when_the_update_stays_on_this_major() {
    let install = Install::at("Cellar/feluda/1.0.0/bin/feluda");
    install.tool("brew", "");
    install.reports_version(&next_minor());
    let base = crate_index(vec![(next_minor(), false)]);

    let output = install.run(&base, &["update"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(install.calls(), vec!["brew upgrade feluda"]);
}

#[test]
fn homebrew_cannot_be_held_to_a_major_so_update_holds_back() {
    let install = Install::at("Cellar/feluda/1.0.0/bin/feluda");
    install.tool("brew", "");
    let base = crate_index(vec![(next_major(), false), (next_minor(), false)]);

    let update = install.run(&base, &["update"]);
    assert!(update.status.success(), "{}", stderr(&update));
    assert_eq!(install.calls(), Vec::<String>::new());
    assert!(stdout(&update).contains("feluda upgrade"));

    install.reports_version(&next_major());
    let upgrade = install.run(&base, &["upgrade"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    assert_eq!(install.calls(), vec!["brew upgrade feluda"]);
}

#[test]
fn a_formula_behind_the_release_is_reported_rather_than_claimed() {
    let install = Install::at("Cellar/feluda/1.0.0/bin/feluda");
    install.tool("brew", "");
    install.reports_version(&current().to_string());
    let base = crate_index(vec![(next_minor(), false)]);

    let output = install.run(&base, &["update"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("is still"), "{text}");
    assert!(!text.contains("Feluda is now"), "{text}");
}

#[test]
fn a_deb_install_downloads_the_package_and_installs_it_with_sudo() {
    let install = Install::at("usr/bin/feluda");
    install.tool("dpkg-query", "");
    install.tool("sudo", r#"for last; do :; done; test -s "$last" || exit 9"#);
    install.reports_version(&next_minor());
    let base = github_releases(vec![next_minor()], vec![]);

    let output = install.run(&base, &["update"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let package = format!("feluda-v{}-{}.deb", next_minor(), std::env::consts::ARCH);
    assert_eq!(
        install.calls().last().expect("no calls"),
        &format!("sudo dpkg -i {package}")
    );
}

#[test]
fn a_release_still_building_its_package_is_passed_over() {
    let install = Install::at("usr/bin/feluda");
    install.tool("dpkg-query", "");
    install.tool("sudo", "");
    let base = github_releases(vec![next_minor(), next_patch()], vec![next_minor()]);

    let output = install.run(&base, &["update", "--check"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains(&format!("New version: v{}", next_patch())));
}

#[test]
fn a_rate_limited_github_says_how_to_raise_the_limit() {
    let install = Install::at("usr/bin/feluda");
    install.tool("dpkg-query", "");

    let output = install.run(&rate_limited(), &["update"]);

    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("GITHUB_TOKEN"), "{text}");
    assert!(!text.contains("Self update error"), "{text}");
}

#[test]
fn without_sudo_a_deb_install_prints_what_to_do() {
    let install = Install::at("usr/bin/feluda");
    install.tool("dpkg-query", "");
    let base = github_releases(vec![next_minor()], vec![]);

    let output = install.run(&base, &["update"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("as root"), "{}", stderr(&output));
}

#[test]
fn an_install_nobody_claims_is_left_alone() {
    let install = Install::at("somewhere/bin/feluda");
    install.tool("cargo", "");
    install.tool("brew", "");
    let base = crate_index(vec![(next_minor(), false)]);

    let output = install.run(&base, &["update"]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(install.calls(), Vec::<String>::new());
    assert!(
        stderr(&output).contains("Can't tell"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_project_config_cannot_redirect_the_release_source() {
    let install = cargo_install();
    let hostile = Path::new(install.root.path()).join(".feluda.toml");
    fs::write(&hostile, "[update]\nindex = \"http://127.0.0.1:1/index\"\n")
        .expect("failed to write .feluda.toml");
    let base = crate_index(vec![(next_minor(), false)]);

    let output = install.run(&base, &["update", "--check"]);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains(&format!("New version: v{}", next_minor())));
}
