//! `feluda update` and `feluda upgrade`: move this install to a newer release through whatever
//! installed it.
//!
//! `update` stays on the installed major version, `upgrade` takes the newest release there is.
//! cargo, `.deb` and `.rpm` installs take an exact version. Homebrew, the AUR and pkgsrc install
//! whatever they offer at the time, so `update` holds back when that could cross a major version,
//! and what actually landed is read back from `feluda --version`.
//!
//! The release sources can be overridden by environment variables only, never `.feluda.toml`: that
//! file comes from the current directory, and a cloned project must not choose what gets installed
//! with `sudo`.

use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use colored::Colorize;
use regex::Regex;
use reqwest::blocking::Response;
use reqwest::StatusCode;
use semver::Version;
use serde::Deserialize;
use tempfile::TempDir;

use crate::command;
use crate::config::timeouts;
use crate::debug::{FeludaError, FeludaResult};
use crate::http_stats;
use crate::licenses::get_github_token;

const CRATE_INDEX: &str = "https://index.crates.io/fe/lu/feluda";
const CRATE_INDEX_ENV: &str = "FELUDA_UPDATE_INDEX";
const GITHUB_RELEASES: &str = "https://api.github.com/repos/anistark/feluda/releases?per_page=100";
const GITHUB_RELEASES_ENV: &str = "FELUDA_UPDATE_RELEASES";

/// How far a run may move the installed version.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scope {
    /// `feluda update`: minor and patch releases of the installed major version.
    SameMajor,
    /// `feluda upgrade`: the newest release, whatever its major version.
    Latest,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum InstallMethod {
    Cargo,
    Homebrew,
    Aur,
    Deb,
    Rpm,
    Pkgsrc,
    Unknown,
}

impl fmt::Display for InstallMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InstallMethod::Cargo => "cargo",
            InstallMethod::Homebrew => "Homebrew",
            InstallMethod::Aur => "the AUR",
            InstallMethod::Deb => "a .deb package",
            InstallMethod::Rpm => "an .rpm package",
            InstallMethod::Pkgsrc => "pkgsrc",
            InstallMethod::Unknown => "an unknown method",
        })
    }
}

impl InstallMethod {
    fn takes_exact_version(self) -> bool {
        matches!(
            self,
            InstallMethod::Cargo | InstallMethod::Deb | InstallMethod::Rpm
        )
    }

    fn package_extension(self) -> Option<&'static str> {
        match self {
            InstallMethod::Deb => Some("deb"),
            InstallMethod::Rpm => Some("rpm"),
            _ => None,
        }
    }
}

fn installed_path() -> PathBuf {
    env::current_exe()
        .and_then(|path| path.canonicalize())
        .unwrap_or_default()
}

fn detect_install_method(
    exe: &Path,
    cargo_home: Option<&str>,
    system_owner: impl FnOnce() -> Option<InstallMethod>,
) -> InstallMethod {
    if exe.to_string_lossy().contains("/Cellar/feluda/") {
        return InstallMethod::Homebrew;
    }
    let in_cargo_home = cargo_home
        .is_some_and(|home| !home.is_empty() && exe.starts_with(Path::new(home).join("bin")));
    if in_cargo_home || exe.parent().is_some_and(|dir| dir.ends_with(".cargo/bin")) {
        return InstallMethod::Cargo;
    }
    if exe.starts_with("/usr/pkg") {
        return InstallMethod::Pkgsrc;
    }
    system_owner().unwrap_or(InstallMethod::Unknown)
}

/// Ask the system package managers whether one of them owns `exe`.
fn system_owner(exe: &Path) -> Option<InstallMethod> {
    let owns = |tool: &str, flag: &str| {
        command::lookup(tool, Command::new(tool).arg(flag).arg(exe))
            .is_ok_and(|output| output.status.success())
    };
    if owns("pacman", "-Qo") {
        Some(InstallMethod::Aur)
    } else if owns("dpkg-query", "-S") {
        Some(InstallMethod::Deb)
    } else if owns("rpm", "-qf") {
        Some(InstallMethod::Rpm)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Package {
    name: String,
    url: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Release {
    version: Version,
    package: Option<Package>,
}

#[derive(Deserialize)]
struct IndexEntry {
    vers: String,
    #[serde(default)]
    yanked: bool,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

fn parse_crate_index(body: &str) -> Vec<Release> {
    body.lines()
        .filter_map(|line| serde_json::from_str::<IndexEntry>(line).ok())
        .filter(|entry| !entry.yanked)
        .filter_map(|entry| Version::parse(&entry.vers).ok())
        .map(|version| Release {
            version,
            package: None,
        })
        .collect()
}

/// A `.deb` or `.rpm` install skips releases with no package for this architecture, which is every
/// release until its build job finishes.
fn parse_github_releases(body: &str, method: InstallMethod) -> FeludaResult<Vec<Release>> {
    let releases: Vec<GithubRelease> =
        serde_json::from_str(body).map_err(|e| FeludaError::SelfUpdate(e.to_string()))?;
    Ok(releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let version = Version::parse(release.tag_name.trim_start_matches('v')).ok()?;
            let package = match method.package_extension() {
                Some(extension) => {
                    let name = format!(
                        "feluda-{}-{}.{extension}",
                        release.tag_name,
                        env::consts::ARCH
                    );
                    let asset = release.assets.into_iter().find(|a| a.name == name)?;
                    Some(Package {
                        name,
                        url: asset.browser_download_url,
                    })
                }
                None => None,
            };
            Some(Release { version, package })
        })
        .collect())
}

fn get(url: &str, token: Option<&str>) -> FeludaResult<Response> {
    let client = http_stats::client_builder().build()?;
    http_stats::send(url, || {
        let request = client.get(url);
        match token {
            Some(token) => request.bearer_auth(token).send(),
            None => request.send(),
        }
    })
}

fn body_of(response: Response) -> FeludaResult<String> {
    let status = response.status();
    if !status.is_success() {
        return Err(FeludaError::SelfUpdate(format!(
            "{} answered {status}",
            response.url()
        )));
    }
    Ok(response.text()?)
}

/// Only `.deb` and `.rpm` installs ask GitHub, for the package files. It allows 60 requests an hour
/// per IP without a token, and the crate index has no such limit, so anything that only needs a
/// version number asks the index.
fn fetch_releases(method: InstallMethod) -> FeludaResult<Vec<Release>> {
    if method.package_extension().is_none() {
        let url = env::var(CRATE_INDEX_ENV).unwrap_or_else(|_| CRATE_INDEX.to_string());
        return Ok(parse_crate_index(&body_of(get(&url, None)?)?));
    }
    let (url, token) = match env::var(GITHUB_RELEASES_ENV) {
        Ok(url) => (url, None),
        Err(_) => (GITHUB_RELEASES.to_string(), get_github_token()),
    };
    let response = get(&url, token)?;
    if matches!(
        response.status(),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) {
        return Err(FeludaError::SelfUpdate(
            "GitHub is limiting requests from this address. Pass `--github-token` or set \
             `GITHUB_TOKEN` to raise the limit"
                .to_string(),
        ));
    }
    parse_github_releases(&body_of(response)?, method)
}

fn newest<'a>(
    releases: &'a [Release],
    current: &Version,
    allowed: impl Fn(&Version) -> bool,
) -> Option<&'a Release> {
    releases
        .iter()
        .filter(|release| {
            let version = &release.version;
            version.pre.is_empty() && version > current && allowed(version)
        })
        .max_by(|a, b| a.version.cmp(&b.version))
}

#[derive(Debug, PartialEq)]
enum Decision<'a> {
    UpToDate,
    /// Only `feluda upgrade` can move this install: the newest release is a new major version, and
    /// either nothing newer exists on this one or the install method cannot be held to it.
    NeedsUpgrade(&'a Version),
    Install {
        target: &'a Release,
        newer_major: Option<&'a Version>,
    },
}

fn decide<'a>(
    releases: &'a [Release],
    current: &Version,
    scope: Scope,
    method: InstallMethod,
) -> Decision<'a> {
    let Some(latest) = newest(releases, current, |_| true) else {
        return Decision::UpToDate;
    };
    if scope == Scope::Latest || latest.version.major == current.major {
        return Decision::Install {
            target: latest,
            newer_major: None,
        };
    }
    let same_major = newest(releases, current, |version| version.major == current.major);
    match same_major {
        Some(target) if method.takes_exact_version() => Decision::Install {
            target,
            newer_major: Some(&latest.version),
        },
        _ => Decision::NeedsUpgrade(&latest.version),
    }
}

#[derive(Debug, PartialEq)]
struct Plan {
    download: Option<Package>,
    commands: Vec<Vec<String>>,
}

impl Plan {
    fn run_only(command: &[&str]) -> Self {
        Plan {
            download: None,
            commands: vec![argv(command)],
        }
    }

    fn describe(&self) {
        if let Some(package) = &self.download {
            println!("Would download: {}", package.url);
        }
        for command in &self.commands {
            println!("Would run: {}", command.join(" ").green().bold());
        }
    }

    /// Commands run from a temporary directory: it holds the download, and it keeps a project's
    /// `rust-toolchain.toml` or `.cargo/config.toml` out of `cargo install`.
    fn run(&self) -> FeludaResult<()> {
        let workdir = TempDir::new()
            .map_err(|e| failure(format!("Could not create a temporary directory: {e}")))?;
        if let Some(package) = &self.download {
            println!("Downloading {}", package.url);
            download(package, workdir.path())
                .map_err(|e| failure(format!("Could not download {}: {e}", package.name)))?;
        }
        for command in &self.commands {
            let line = command.join(" ");
            println!("Running: {}", line.green().bold());
            let status = command::interactive(
                Command::new(&command[0])
                    .args(&command[1..])
                    .current_dir(workdir.path()),
            )
            .map_err(|e| failure(format!("Could not run `{line}`: {e}")))?;
            if !status.success() {
                return Err(failure(format!("`{line}` failed ({status})")));
            }
        }
        Ok(())
    }
}

fn download(package: &Package, dir: &Path) -> FeludaResult<()> {
    let client = http_stats::client_builder()
        .timeout(Duration::from_secs(timeouts().command))
        .build()?;
    let response = http_stats::send(&package.url, || client.get(&package.url).send())?;
    let status = response.status();
    if !status.is_success() {
        return Err(FeludaError::SelfUpdate(format!(
            "{} answered {status}",
            package.url
        )));
    }
    fs::write(dir.join(&package.name), response.bytes()?)?;
    Ok(())
}

fn argv(command: &[&str]) -> Vec<String> {
    command.iter().map(|arg| arg.to_string()).collect()
}

fn on_path(tool: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| dir.join(tool).is_file()))
}

fn is_root() -> bool {
    command::lookup("id", Command::new("id").arg("-u"))
        .is_ok_and(|output| output.stdout.trim_ascii() == b"0")
}

/// `command` as the current user can run it with root's rights, or None when they cannot.
fn as_root(command: &[&str]) -> Option<Vec<String>> {
    if is_root() {
        Some(argv(command))
    } else if on_path("sudo") {
        let mut elevated = argv(&["sudo"]);
        elevated.extend(argv(command));
        Some(elevated)
    } else {
        None
    }
}

/// What to run for this install, or instructions for the user when feluda cannot do it itself.
fn plan_for(method: InstallMethod, target: &Release) -> Result<Plan, String> {
    match method {
        InstallMethod::Cargo => {
            let version = target.version.to_string();
            let command = [
                "cargo",
                "install",
                "feluda",
                "--version",
                &version,
                "--locked",
            ];
            if cfg!(windows) {
                return Err(format!(
                    "Windows cannot replace a program while it runs, so run this yourself: {}",
                    command.join(" ")
                ));
            }
            Ok(Plan::run_only(&command))
        }
        InstallMethod::Homebrew => Ok(Plan::run_only(&["brew", "upgrade", "feluda"])),
        InstallMethod::Aur => ["paru", "yay"]
            .into_iter()
            .find(|helper| on_path(helper))
            .map(|helper| Plan::run_only(&[helper, "-S", "feluda"]))
            .ok_or_else(|| {
                "No AUR helper (paru or yay) found. Rebuild the package from \
                 https://aur.archlinux.org/packages/feluda"
                    .to_string()
            }),
        InstallMethod::Deb | InstallMethod::Rpm => {
            let package = target
                .package
                .clone()
                .ok_or_else(|| format!("v{} has no package to install", target.version))?;
            let install = match method {
                InstallMethod::Deb => ["dpkg", "-i", &package.name],
                _ => ["rpm", "-Uvh", &package.name],
            };
            let command = as_root(&install).ok_or_else(|| {
                format!(
                    "Download {} and run this as root: {}",
                    package.url,
                    install.join(" ")
                )
            })?;
            Ok(Plan {
                download: Some(package),
                commands: vec![command],
            })
        }
        InstallMethod::Pkgsrc => {
            let install = ["pkgin", "install", "feluda"];
            as_root(&install)
                .map(|command| Plan {
                    download: None,
                    commands: vec![command],
                })
                .ok_or_else(|| format!("Run this as root: {}", install.join(" ")))
        }
        InstallMethod::Unknown => Err(
            "Can't tell how this copy of Feluda was installed, so it was left as it is. \
             Reinstall it the way you installed it, or with `cargo install feluda`."
                .to_string(),
        ),
    }
}

fn version_in_banner(banner: &str) -> Option<Version> {
    let pattern = Regex::new(r"Feluda v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)").ok()?;
    Version::parse(pattern.captures(banner)?.get(1)?.as_str()).ok()
}

/// What typing `feluda` gives now. The path this process started from can be gone by this point:
/// `brew upgrade` removes the old Cellar directory.
fn version_on_path() -> Option<Version> {
    let output =
        command::lookup("feluda --version", Command::new("feluda").arg("--version")).ok()?;
    version_in_banner(&String::from_utf8_lossy(&output.stdout))
}

fn report_outcome(target: &Version, method: InstallMethod) {
    match version_on_path() {
        Some(version) if version >= *target => {
            println!("{} Feluda is now v{version}", "✓".green());
        }
        Some(version) => println!(
            "{} `feluda` is still v{version}. {method} may not offer v{target} yet, or another \
             copy comes first on PATH.",
            "⚠".yellow()
        ),
        None => println!(
            "{} Finished. Check the result with `feluda --version`.",
            "✓".green()
        ),
    }
}

fn failure(message: String) -> FeludaError {
    eprintln!("❌ {message}");
    FeludaError::SelfUpdate(message)
}

fn install(target: &Release, method: InstallMethod, check: bool) -> FeludaResult<()> {
    println!("{}", format!("New version: v{}", target.version).yellow());
    let plan = plan_for(method, target);
    if check {
        match plan {
            Ok(plan) => plan.describe(),
            Err(instructions) => println!("{instructions}"),
        }
        return Ok(());
    }
    plan.map_err(failure)?.run()?;
    report_outcome(&target.version, method);
    Ok(())
}

pub fn handle_self_update(scope: Scope, check: bool) -> FeludaResult<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| FeludaError::SelfUpdate(e.to_string()))?;
    let exe = installed_path();
    let cargo_home = env::var("CARGO_HOME").ok();
    let method = detect_install_method(&exe, cargo_home.as_deref(), || system_owner(&exe));
    println!(
        "Feluda v{current}, installed with {method} ({})",
        exe.display()
    );

    let releases = fetch_releases(method)
        .map_err(|e| failure(format!("Could not read the published releases: {e}")))?;

    match decide(&releases, &current, scope, method) {
        Decision::UpToDate => println!("{} You're on the latest version", "✓".green()),
        Decision::NeedsUpgrade(latest) => {
            if !method.takes_exact_version() {
                println!(
                    "{method} only installs its newest version, so it can't stay on v{}.",
                    current.major
                );
            }
            println!("v{latest} is a new major version. Move to it with `feluda upgrade`.");
        }
        Decision::Install {
            target,
            newer_major,
        } => {
            install(target, method, check)?;
            if let Some(latest) = newer_major {
                println!("v{latest} is a new major version. Move to it with `feluda upgrade`.");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> Release {
        Release {
            version: Version::parse(version).unwrap(),
            package: None,
        }
    }

    fn releases(versions: &[&str]) -> Vec<Release> {
        versions.iter().map(|version| release(version)).collect()
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn detect(exe: &str, cargo_home: Option<&str>, owner: Option<InstallMethod>) -> InstallMethod {
        detect_install_method(Path::new(exe), cargo_home, || owner)
    }

    #[test]
    fn homebrew_is_read_from_the_cellar_on_every_prefix() {
        for exe in [
            "/opt/homebrew/Cellar/feluda/1.14.0/bin/feluda",
            "/usr/local/Cellar/feluda/1.14.0/bin/feluda",
            "/home/linuxbrew/.linuxbrew/Cellar/feluda/1.14.0/bin/feluda",
        ] {
            assert_eq!(detect(exe, None, None), InstallMethod::Homebrew);
        }
    }

    #[test]
    fn a_checkout_is_not_mistaken_for_a_package_manager() {
        for exe in [
            "/home/dev/homebrew-tools/target/release/feluda",
            "/home/dev/.cargo/registry/feluda/target/release/feluda",
            "/usr/local/bin/feluda",
        ] {
            assert_eq!(detect(exe, None, None), InstallMethod::Unknown);
        }
    }

    #[test]
    fn cargo_is_read_from_its_bin_directory() {
        assert_eq!(
            detect("/Users/dev/.cargo/bin/feluda", None, None),
            InstallMethod::Cargo
        );
        assert_eq!(
            detect("/custom/rust/bin/feluda", Some("/custom/rust"), None),
            InstallMethod::Cargo
        );
        assert_eq!(
            detect("/custom/rustup/bin/feluda", Some("/custom/rust"), None),
            InstallMethod::Unknown
        );
    }

    #[test]
    fn a_system_package_manager_is_believed_when_it_owns_the_file() {
        for method in [InstallMethod::Aur, InstallMethod::Deb, InstallMethod::Rpm] {
            assert_eq!(detect("/usr/bin/feluda", None, Some(method)), method);
        }
        assert_eq!(
            detect("/usr/bin/feluda", None, None),
            InstallMethod::Unknown
        );
        assert_eq!(
            detect("/usr/pkg/bin/feluda", None, None),
            InstallMethod::Pkgsrc
        );
    }

    #[test]
    fn the_crate_index_skips_yanked_versions() {
        let body = concat!(
            r#"{"name":"feluda","vers":"1.0.0","yanked":false}"#,
            "\n",
            r#"{"name":"feluda","vers":"1.1.0","yanked":true}"#,
            "\n",
            "not json\n",
        );
        assert_eq!(parse_crate_index(body), releases(&["1.0.0"]));
    }

    #[test]
    fn github_drafts_and_prereleases_are_skipped() {
        let body = r#"[
            {"tag_name":"v2.0.0","draft":true},
            {"tag_name":"v1.9.0","prerelease":true},
            {"tag_name":"v1.8.0"},
            {"tag_name":"nightly"}
        ]"#;
        assert_eq!(
            parse_github_releases(body, InstallMethod::Homebrew).unwrap(),
            releases(&["1.8.0"])
        );
    }

    #[test]
    fn a_package_install_needs_a_package_for_this_architecture() {
        let arch = env::consts::ARCH;
        let body = format!(
            r#"[
                {{"tag_name":"v1.9.0","assets":[
                    {{"name":"feluda-v1.9.0-other.deb","browser_download_url":"https://x/other"}}
                ]}},
                {{"tag_name":"v1.8.0","assets":[
                    {{"name":"feluda-v1.8.0-{arch}.rpm","browser_download_url":"https://x/rpm"}},
                    {{"name":"feluda-v1.8.0-{arch}.deb","browser_download_url":"https://x/deb"}}
                ]}}
            ]"#
        );
        let found = parse_github_releases(&body, InstallMethod::Deb).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version, version("1.8.0"));
        assert_eq!(found[0].package.as_ref().unwrap().url, "https://x/deb");
    }

    #[test]
    fn update_takes_the_newest_release_of_the_installed_major() {
        let published = releases(&["1.18.0", "1.18.1", "1.19.0", "2.0.0"]);
        assert_eq!(
            decide(
                &published,
                &version("1.18.0"),
                Scope::SameMajor,
                InstallMethod::Cargo
            ),
            Decision::Install {
                target: &published[2],
                newer_major: Some(&published[3].version),
            }
        );
    }

    #[test]
    fn upgrade_takes_the_newest_release_there_is() {
        let published = releases(&["1.19.0", "2.0.0", "2.1.0"]);
        assert_eq!(
            decide(
                &published,
                &version("1.18.0"),
                Scope::Latest,
                InstallMethod::Cargo
            ),
            Decision::Install {
                target: &published[2],
                newer_major: None,
            }
        );
    }

    #[test]
    fn upgrade_still_takes_a_minor_release_when_no_major_is_out() {
        let published = releases(&["1.19.0"]);
        assert_eq!(
            decide(
                &published,
                &version("1.18.0"),
                Scope::Latest,
                InstallMethod::Homebrew
            ),
            Decision::Install {
                target: &published[0],
                newer_major: None,
            }
        );
    }

    #[test]
    fn update_stops_at_a_new_major_when_nothing_else_is_newer() {
        let published = releases(&["1.18.0", "2.0.0"]);
        assert_eq!(
            decide(
                &published,
                &version("1.18.0"),
                Scope::SameMajor,
                InstallMethod::Cargo
            ),
            Decision::NeedsUpgrade(&published[1].version)
        );
    }

    #[test]
    fn update_holds_back_when_the_install_method_would_cross_a_major() {
        let published = releases(&["1.19.0", "2.0.0"]);
        assert_eq!(
            decide(
                &published,
                &version("1.18.0"),
                Scope::SameMajor,
                InstallMethod::Homebrew
            ),
            Decision::NeedsUpgrade(&published[1].version)
        );
    }

    #[test]
    fn nothing_newer_means_up_to_date() {
        let published = releases(&["1.17.0", "1.18.0", "1.19.0-rc.1"]);
        for scope in [Scope::SameMajor, Scope::Latest] {
            assert_eq!(
                decide(&published, &version("1.18.0"), scope, InstallMethod::Cargo),
                Decision::UpToDate
            );
        }
    }

    #[test]
    fn a_prerelease_build_moves_to_its_final_release() {
        let published = releases(&["1.19.0"]);
        assert_eq!(
            decide(
                &published,
                &version("1.19.0-rc.1"),
                Scope::SameMajor,
                InstallMethod::Cargo
            ),
            Decision::Install {
                target: &published[0],
                newer_major: None,
            }
        );
    }

    #[test]
    fn cargo_is_pinned_to_the_chosen_version() {
        let plan = plan_for(InstallMethod::Cargo, &release("1.19.0"));
        if cfg!(windows) {
            assert!(plan.unwrap_err().contains("cargo install feluda"));
        } else {
            assert_eq!(
                plan.unwrap().commands,
                vec![argv(&[
                    "cargo",
                    "install",
                    "feluda",
                    "--version",
                    "1.19.0",
                    "--locked"
                ])]
            );
        }
    }

    #[test]
    fn an_unknown_install_gets_instructions_and_no_command() {
        let instructions = plan_for(InstallMethod::Unknown, &release("1.19.0")).unwrap_err();
        assert!(instructions.contains("cargo install feluda"));
    }

    #[test]
    fn the_version_is_read_out_of_the_banner() {
        let banner = "  ▌▖▖▖   Feluda v1.19.0\n  A dependency license checker";
        assert_eq!(version_in_banner(banner), Some(version("1.19.0")));
        let colored = "\u{1b}[1;97mFeluda v2.0.0-rc.1\u{1b}[0m";
        assert_eq!(version_in_banner(colored), Some(version("2.0.0-rc.1")));
        assert_eq!(version_in_banner("feluda: command not found"), None);
    }
}
