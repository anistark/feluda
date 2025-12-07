use regex::Regex;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::FeludaConfig;
use crate::debug::{log, log_debug, log_error, LogLevel};
use crate::licenses::{
    fetch_licenses_from_github, is_license_restrictive, LicenseCompatibility, LicenseInfo,
};

#[derive(Debug, Clone)]
pub struct NuGetPackage {
    pub name: String,
    pub version: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct PackagesLockJson {
    version: i32,
    dependencies: Option<HashMap<String, HashMap<String, PackageLockInfo>>>,
}

#[derive(Deserialize, Serialize, Debug)]
struct PackageLockInfo {
    #[serde(rename = "type")]
    package_type: Option<String>,
    resolved: Option<String>,
    #[serde(rename = "contentHash")]
    content_hash: Option<String>,
    dependencies: Option<HashMap<String, String>>,
}

pub fn analyze_dotnet_licenses(project_path: &str, config: &FeludaConfig) -> Vec<LicenseInfo> {
    log(
        LogLevel::Info,
        &format!("Analyzing .NET dependencies from: {project_path}"),
    );

    let known_licenses = match fetch_licenses_from_github() {
        Ok(licenses) => {
            log(
                LogLevel::Info,
                &format!("Fetched {} known licenses from GitHub", licenses.len()),
            );
            licenses
        }
        Err(err) => {
            log_error("Failed to fetch licenses from GitHub", &err);
            HashMap::new()
        }
    };

    let direct_deps = match detect_and_parse_project(project_path) {
        Ok(deps) => deps,
        Err(err) => {
            log_error("Failed to parse .NET project", &err);
            return Vec::new();
        }
    };

    log(
        LogLevel::Info,
        &format!("Found {} direct .NET dependencies", direct_deps.len()),
    );
    log_debug("Direct .NET dependencies", &direct_deps);

    let max_depth = config.dependencies.max_depth;
    log(
        LogLevel::Info,
        &format!("Using max dependency depth: {max_depth}"),
    );

    let all_deps = resolve_dotnet_dependencies(project_path, &direct_deps, max_depth);

    let mut licenses = Vec::new();
    for (name, version) in all_deps {
        log(
            LogLevel::Info,
            &format!("Processing dependency: {name} ({version})"),
        );

        let license_result = fetch_license_for_nuget_package(&name, &version);
        let license = Some(license_result);
        let is_restrictive = is_license_restrictive(&license, &known_licenses, config.strict);

        if is_restrictive {
            log(
                LogLevel::Warn,
                &format!("Restrictive license found: {license:?} for {name}"),
            );
        }

        licenses.push(LicenseInfo {
            name,
            version,
            license: license.clone(),
            is_restrictive,
            compatibility: LicenseCompatibility::Unknown,
            osi_status: match &license {
                Some(l) => crate::licenses::get_osi_status(l),
                None => crate::licenses::OsiStatus::Unknown,
            },
        });
    }

    log(
        LogLevel::Info,
        &format!("Found {} .NET dependencies with licenses", licenses.len()),
    );
    licenses
}

fn detect_and_parse_project(project_path: &str) -> Result<Vec<NuGetPackage>, String> {
    let path = Path::new(project_path);

    if path.extension().and_then(|s| s.to_str()) == Some("slnx") {
        parse_slnx_solution(project_path)
    } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        if ext == "csproj" || ext == "fsproj" || ext == "vbproj" {
            parse_csproj_file(project_path)
        } else {
            Err(format!("Unsupported .NET file type: {ext}"))
        }
    } else {
        let parent_dir = path.parent().unwrap_or(path);
        if let Ok(lock_path) = find_file_in_dir(parent_dir, "packages.lock.json") {
            parse_packages_lock_json(&lock_path)
        } else {
            parse_csproj_file(project_path)
        }
    }
}

fn parse_slnx_solution(slnx_path: &str) -> Result<Vec<NuGetPackage>, String> {
    log(LogLevel::Info, &format!("Parsing .slnx file: {slnx_path}"));

    let content =
        fs::read_to_string(slnx_path).map_err(|e| format!("Failed to read .slnx file: {e}"))?;

    let re = Regex::new(r#"<Project\s+Path="([^"]+)""#)
        .map_err(|e| format!("Failed to compile regex: {e}"))?;

    let slnx_dir = Path::new(slnx_path)
        .parent()
        .ok_or("Failed to get parent directory")?;

    let mut all_packages = Vec::new();
    let mut seen = HashSet::new();

    for cap in re.captures_iter(&content) {
        let project_rel_path = &cap[1];
        let normalized_path = project_rel_path.replace('\\', "/");
        let project_path = slnx_dir.join(normalized_path);

        log(
            LogLevel::Info,
            &format!("Found project in solution: {}", project_path.display()),
        );

        if let Ok(project_path_str) = project_path.to_str().ok_or("Invalid path") {
            match parse_csproj_file(project_path_str) {
                Ok(packages) => {
                    for pkg in packages {
                        let key = format!("{}@{}", pkg.name, pkg.version);
                        if seen.insert(key) {
                            all_packages.push(pkg);
                        }
                    }
                }
                Err(e) => log_error(
                    &format!("Failed to parse project: {}", project_path.display()),
                    &e,
                ),
            }
        }
    }

    log(
        LogLevel::Info,
        &format!("Found {} total packages from solution", all_packages.len()),
    );
    Ok(all_packages)
}

fn parse_csproj_file(csproj_path: &str) -> Result<Vec<NuGetPackage>, String> {
    log(
        LogLevel::Info,
        &format!("Parsing .csproj file: {csproj_path}"),
    );

    let content =
        fs::read_to_string(csproj_path).map_err(|e| format!("Failed to read .csproj file: {e}"))?;

    let mut packages = Vec::new();

    let pkg_re = Regex::new(r#"<PackageReference\s+Include="([^"]+)"\s+Version="([^"]+)"\s*/?>"#)
        .map_err(|e| format!("Failed to compile regex: {e}"))?;

    for cap in pkg_re.captures_iter(&content) {
        let name = cap[1].to_string();
        let version = cap[2].to_string();
        packages.push(NuGetPackage { name, version });
    }

    let csproj_dir = Path::new(csproj_path)
        .parent()
        .ok_or("Failed to get parent directory")?;

    if let Ok(project_refs) = parse_project_references(&content, csproj_dir) {
        for ref_path in project_refs {
            match parse_csproj_file(&ref_path) {
                Ok(ref_packages) => packages.extend(ref_packages),
                Err(e) => log_error(
                    &format!("Failed to parse project reference: {ref_path}"),
                    &e,
                ),
            }
        }
    }

    log(
        LogLevel::Info,
        &format!("Found {} packages in .csproj", packages.len()),
    );
    Ok(packages)
}

fn parse_project_references(content: &str, base_dir: &Path) -> Result<Vec<String>, String> {
    let re = Regex::new(r#"<ProjectReference\s+Include="([^"]+)"\s*/?>"#)
        .map_err(|e| format!("Failed to compile regex: {e}"))?;

    let mut references = Vec::new();
    for cap in re.captures_iter(content) {
        let rel_path = &cap[1];
        let normalized_path = rel_path.replace('\\', "/");
        let abs_path = base_dir.join(normalized_path);
        if let Some(path_str) = abs_path.to_str() {
            references.push(path_str.to_string());
        }
    }

    Ok(references)
}

fn parse_packages_lock_json(lock_path: &str) -> Result<Vec<NuGetPackage>, String> {
    log(
        LogLevel::Info,
        &format!("Parsing packages.lock.json: {lock_path}"),
    );

    let content = fs::read_to_string(lock_path)
        .map_err(|e| format!("Failed to read packages.lock.json: {e}"))?;

    let lock_data: PackagesLockJson = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse packages.lock.json: {e}"))?;

    let mut packages = Vec::new();

    if let Some(dependencies) = lock_data.dependencies {
        for (_framework, packages_map) in dependencies {
            for (name, info) in packages_map {
                if let Some(resolved) = &info.resolved {
                    packages.push(NuGetPackage {
                        name: name.clone(),
                        version: resolved.clone(),
                    });
                }
            }
        }
    }

    log(
        LogLevel::Info,
        &format!("Found {} packages in lock file", packages.len()),
    );
    Ok(packages)
}

fn resolve_dotnet_dependencies(
    project_path: &str,
    direct_deps: &[NuGetPackage],
    max_depth: u32,
) -> Vec<(String, String)> {
    if max_depth == 0 {
        return direct_deps
            .iter()
            .map(|p| (p.name.clone(), p.version.clone()))
            .collect();
    }

    match resolve_with_dotnet_list(project_path) {
        Ok(deps) => deps,
        Err(e) => {
            log_error(
                "Failed to resolve with dotnet list, using direct dependencies",
                &e,
            );
            direct_deps
                .iter()
                .map(|p| (p.name.clone(), p.version.clone()))
                .collect()
        }
    }
}

fn resolve_with_dotnet_list(project_path: &str) -> Result<Vec<(String, String)>, String> {
    log(
        LogLevel::Info,
        "Attempting to resolve dependencies with dotnet list package",
    );

    let path = Path::new(project_path);
    let work_dir = if path.is_dir() {
        path
    } else {
        path.parent().ok_or("Failed to get parent directory")?
    };

    let output = Command::new("dotnet")
        .args(["list", "package", "--include-transitive"])
        .current_dir(work_dir)
        .output()
        .map_err(|e| format!("Failed to execute dotnet command: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "dotnet list package failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_dotnet_list_output(&stdout)
}

fn parse_dotnet_list_output(output: &str) -> Result<Vec<(String, String)>, String> {
    let re =
        Regex::new(r">\s+(\S+)\s+(\S+)").map_err(|e| format!("Failed to compile regex: {e}"))?;

    let mut packages = Vec::new();
    for cap in re.captures_iter(output) {
        let name = cap[1].to_string();
        let version = cap[2].to_string();
        packages.push((name, version));
    }

    log(
        LogLevel::Info,
        &format!("Parsed {} packages from dotnet list output", packages.len()),
    );
    Ok(packages)
}

fn fetch_license_for_nuget_package(name: &str, version: &str) -> String {
    if let Ok(license) = fetch_from_local_nuget_cache(name, version) {
        return license;
    }

    if let Ok(license) = fetch_from_nuget_api(name, version) {
        return license;
    }

    log(
        LogLevel::Warn,
        &format!("Could not find license for {name} {version}"),
    );
    "Unknown".to_string()
}

fn fetch_from_local_nuget_cache(name: &str, version: &str) -> Result<String, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "Cannot determine home directory")?;

    let nuget_cache = PathBuf::from(home)
        .join(".nuget")
        .join("packages")
        .join(name.to_lowercase())
        .join(version)
        .join(format!("{}.nuspec", name.to_lowercase()));

    if nuget_cache.exists() {
        let content =
            fs::read_to_string(&nuget_cache).map_err(|e| format!("Failed to read nuspec: {e}"))?;
        return parse_license_from_nuspec(&content);
    }

    Err("Not found in local cache".to_string())
}

fn fetch_from_nuget_api(name: &str, version: &str) -> Result<String, String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let nuspec_url = format!(
        "https://api.nuget.org/v3-flatcontainer/{}/{}/{}.nuspec",
        name.to_lowercase(),
        version.to_lowercase(),
        name.to_lowercase()
    );

    log(
        LogLevel::Info,
        &format!("Fetching from NuGet: {nuspec_url}"),
    );

    let response = client
        .get(&nuspec_url)
        .send()
        .map_err(|e| format!("Failed to fetch nuspec: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("NuGet API returned status: {}", response.status()));
    }

    let content = response
        .text()
        .map_err(|e| format!("Failed to read response: {e}"))?;

    parse_license_from_nuspec(&content)
}

fn parse_license_from_nuspec(content: &str) -> Result<String, String> {
    if let Ok(re) = Regex::new(r"<license[^>]*>([^<]+)</license>") {
        if let Some(cap) = re.captures(content) {
            return Ok(cap[1].trim().to_string());
        }
    }

    if let Ok(re) = Regex::new(r#"<licenseUrl>([^<]+)</licenseUrl>"#) {
        if let Some(cap) = re.captures(content) {
            let url = cap[1].trim();
            if url.contains("MIT") {
                return Ok("MIT".to_string());
            } else if url.contains("Apache") {
                return Ok("Apache-2.0".to_string());
            } else if url.contains("BSD") {
                return Ok("BSD".to_string());
            } else if url.contains("GPL") {
                return Ok("GPL".to_string());
            }
            return Ok(url.to_string());
        }
    }

    Err("No license found in nuspec".to_string())
}

fn find_file_in_dir(dir: &Path, filename: &str) -> Result<String, String> {
    let file_path = dir.join(filename);
    if file_path.exists() {
        file_path
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Invalid path".to_string())
    } else {
        Err(format!("{filename} not found in directory"))
    }
}
