use cargo_metadata::Package;
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;
use std::fs::File;
use std::io::{self, BufRead};
use scraper::{Html, Selector};

#[derive(Serialize, Debug)]
pub struct LicenseInfo {
    pub name: String,
    pub version: String,
    pub license: Option<String>,
    pub is_restrictive: bool,
}

impl LicenseInfo {
    pub fn get_license(&self) -> String {
        match &self.license {
            Some(license_name) => {
                String::from(license_name)
            }
            None => {
                String::from("No License")
            }
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn is_restrictive(&self) -> &bool {
        &self.is_restrictive
    }
}

pub fn analyze_rust_licenses(packages: Vec<Package>) -> Vec<LicenseInfo> {
    packages
        .into_iter()
        .map(|package| {
            let is_restrictive = match &package.license {
                Some(license) if license.contains("GPL") || license.contains("AGPL") => true,
                _ => false,
            };

            LicenseInfo {
                name: package.name,
                version: package.version.to_string(),
                license: package.license,
                is_restrictive,
            }
        })
        .collect()
}

#[derive(Deserialize, Serialize, Debug)]
struct PackageJson {
    dependencies: Option<std::collections::HashMap<String, String>>,
    dev_dependencies: Option<std::collections::HashMap<String, String>>,
}

pub fn analyze_js_licenses(package_json_path: &str) -> Vec<LicenseInfo> {
    let content = fs::read_to_string(package_json_path)
        .expect("Failed to read package.json file");
    let package_json: PackageJson = serde_json::from_str(&content)
        .expect("Failed to parse package.json");

    let mut licenses = Vec::new();

    let mut process_deps = |deps: Option<std::collections::HashMap<String, String>>| {
        if let Some(deps) = deps {
            for (name, version) in &deps {
                let output = Command::new("npm")
                    .arg("view")
                    .arg(name)
                    .arg("version")
                    .arg(version)
                    .arg("license")
                    .output()
                    .expect("Failed to execute npm command");

                let output_str = String::from_utf8_lossy(&output.stdout);
                let license = output_str
                    .lines()
                    .find(|line| line.starts_with("license ="))
                    .map(|line| line.replace("license =", "").replace("\'", "").trim().to_string())
                    .unwrap_or_else(|| "No License".to_string());
                let is_restrictive = license.contains("GPL") || license.contains("AGPL");

                licenses.push(LicenseInfo {
                    name: name.clone(),
                    version: version.clone(),
                    license: Some(license),
                    is_restrictive,
                });
            }
        }
    };

    process_deps(package_json.dependencies);
    process_deps(package_json.dev_dependencies);

    licenses   
}

pub fn analyze_go_licenses(go_mod_path: &str) -> Vec<LicenseInfo> {
    let file = File::open(go_mod_path).expect("Failed to open go.mod file");
    let reader = io::BufReader::new(file);

    let mut licenses = Vec::new();
    let mut in_require_block = false;

    for line in reader.lines() {
        let line = line.expect("Failed to read line");
        if line.starts_with("require (") {
            in_require_block = true;
            continue;
        } else if line.starts_with(")") {
            in_require_block = false;
            continue;
        }
        if in_require_block || line.starts_with("require") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[0].to_string();
                let version = parts[1].to_string();
                let license = Some(fetch_license_for_go_dependency(&name, &version));
                // println!("{}: {}", name, license.as_ref().unwrap());
                let is_restrictive = false;

                licenses.push(LicenseInfo {
                    name,
                    version,
                    license,
                    is_restrictive,
                });
            }
        }
    }

    licenses
}

/// Fetch the license for a Go dependency from the Go Package Index (pkg.go.dev)
pub fn fetch_license_for_go_dependency(name: &str, _version: &str) -> String {
    // Format the URL for the Go package metadata
    let api_url = format!("https://pkg.go.dev/{}/", name);

    // Make a GET request to fetch the metadata
    match reqwest::blocking::get(&api_url) {
        Ok(response) => {
            if response.status().is_success() {
                // Parse the HTML to extract license information
                if let Ok(html_content) = response.text() {
                    if let Some(license) = extract_license_from_html(&html_content) {
                        return license;
                    }
                }
            }
        }
        Err(err) => eprintln!("Failed to fetch metadata for {}: {}", name, err),
    }

    // Default to "Unknown" if license could not be fetched
    "Unknown".to_string()
}

/// Extract license information from the HTML content
fn extract_license_from_html(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let span_selector = Selector::parse(r#"span.go-Main-headerDetailItem[data-test-id="UnitHeader-licenses"]"#).unwrap();
    let a_selector = Selector::parse(r#"a[data-test-id="UnitHeader-license"]"#).unwrap();

    if let Some(span_element) = document.select(&span_selector).next() {
        if let Some(a_element) = span_element.select(&a_selector).next() {
            return Some(a_element.text().collect::<Vec<_>>().join(" ").trim().to_string());
        }
    }
    None
}
