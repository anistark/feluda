use cargo_metadata::Package;
use rayon::prelude::*;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead};
use std::process::Command;

// This is used to deserialize the license files from the choosealicense.com repository
#[derive(Debug, Deserialize, Serialize)]
struct License {
    title: String,            // The full name of the license
    spdx_id: String,          // The SPDX identifier for the license
    permissions: Vec<String>, // A list of permissions granted by the license
    conditions: Vec<String>,  // A list of conditions that must be met under the license
    limitations: Vec<String>, // A list of limitations imposed by the license
}

// This struct is used to store information about the licenses of dependencies
#[derive(Serialize, Debug)]
pub struct LicenseInfo {
    pub name: String,            // The name of the software or library
    pub version: String,         // The version of the software or library
    pub license: Option<String>, // An optional field that contains the license type (e.g., MIT, Apache 2.0)
    pub is_restrictive: bool,    // A boolean indicating whether the license is restrictive or not
}

const RESTRICTIVE_LICENSES: &[&str] = &[
    "GPL-3.0",
    "AGPL-3.0",
    "LGPL-3.0",
    "MPL-2.0",
    "SEE LICENSE IN LICENSE",
    "CC-BY-SA-4.0",
    "EPL-2.0",
];

impl LicenseInfo {
    pub fn get_license(&self) -> String {
        match &self.license {
            Some(license_name) => String::from(license_name),
            None => String::from("No License"),
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
    if packages.is_empty() {
        return vec![];
    }

    if packages.is_empty() {
        return vec![];
    }
    let known_licenses = fetch_licenses_from_github();
    packages
        .par_iter()
        .map(|package| {
            let is_restrictive = is_license_restrictive(&package.license, &known_licenses);

            LicenseInfo {
                name: package.name.clone(),
                version: package.version.to_string(),
                license: package.license.clone(),
                is_restrictive,
            }
        })
        .collect()
}

#[derive(Deserialize, Serialize, Debug)]
struct PackageJson {
    dependencies: Option<HashMap<String, String>>,
    dev_dependencies: Option<HashMap<String, String>>,
}

impl PackageJson {
    fn get_all_dependencies(self) -> HashMap<String, String> {
        let mut all_dependencies: HashMap<String, String> = HashMap::new();
        match self.dev_dependencies {
            Some(deps) => all_dependencies.extend(deps),
            None => (),
        };
        match self.dependencies {
            Some(deps) => all_dependencies.extend(deps),
            None => (),
        };
        all_dependencies
    }
}

/// Analyze the licenses of Python dependencies
pub fn analyze_python_licenses(requirements_txt_path: &str) -> Vec<LicenseInfo> {
    let file = File::open(requirements_txt_path).expect("Failed to open requirements.txt file");
    let reader = io::BufReader::new(file);

    let mut licenses = Vec::new();

    let known_licenses = fetch_licenses_from_github();
    for line in reader.lines() {
        let line = line.expect("Failed to read line");
        let parts: Vec<&str> = line.split("==").collect();
        if parts.len() >= 2 {
            let name = parts[0].to_string();
            let version = parts[1].to_string();
            let license = Some(fetch_license_for_python_dependency(&name, &version));
            let is_restrictive = is_license_restrictive(&license, &known_licenses);

            licenses.push(LicenseInfo {
                name,
                version,
                license,
                is_restrictive,
            });
        }
    }

    licenses
}

/// Analyze the licenses of JavaScript dependencies
pub fn analyze_js_licenses(package_json_path: &str) -> Vec<LicenseInfo> {
    let content = fs::read_to_string(package_json_path).expect("Failed to read package.json file");
    let package_json: PackageJson =
        serde_json::from_str(&content).expect("Failed to parse package.json");
    let all_dependencies = package_json.get_all_dependencies();
    let known_licenses = fetch_licenses_from_github();

    all_dependencies
        .par_iter()
        .map(|(name, version)| {
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
                .map(|line| {
                    line.replace("license =", "")
                        .replace("\'", "")
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| "No License".to_string());
            let is_restrictive = is_license_restrictive(&Some(license.clone()), &known_licenses);

            LicenseInfo {
                name: name.clone(),
                version: version.clone(),
                license: Some(license),
                is_restrictive,
            }
        })
        .collect()
}

/// Analyze the licenses of Go dependencies
pub fn analyze_go_licenses(go_mod_path: &str) -> Vec<LicenseInfo> {
    let file = File::open(go_mod_path).expect("Failed to open go.mod file");
    let reader = io::BufReader::new(file);

    let mut licenses = Vec::new();
    let mut in_require_block = false;

    let known_licenses = fetch_licenses_from_github();

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
                let is_restrictive = is_license_restrictive(&license, &known_licenses);

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

/// Fetch the license for a Python dependency from the Python Package Index (PyPI)
pub fn fetch_license_for_python_dependency(name: &str, version: &str) -> String {
    let api_url = format!("https://pypi.org/pypi/{}/{}/json", name, version);
    match reqwest::blocking::get(&api_url) {
        Ok(response) => {
            if response.status().is_success() {
                // Parse the HTML to extract license information
                if let Ok(json) = response.json::<Value>() {
                    let license = json["info"]["license"]
                        .as_str()
                        .map(|s| s.to_string())
                        .expect("No license found");
                    if license.is_empty() {
                        eprintln!("No license found for {}: {}", name, version);
                        format!("Unknown license for {}: {}", name, version)
                    } else {
                        license
                    }
                } else {
                    eprintln!("Failed to parse JSON for {}: {}", name, version);
                    String::from("Unknown")
                }
            } else {
                eprintln!("Failed to fetch metadata for {}: {}", name, version);
                String::from("Unknown")
            }
        }
        Err(err) => {
            eprintln!("Failed to fetch metadata for {}: {}", name, err);
            String::from("")
        }
    }
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
    let span_selector =
        Selector::parse(r#"span.go-Main-headerDetailItem[data-test-id="UnitHeader-licenses"]"#)
            .unwrap();
    let a_selector = Selector::parse(r#"a[data-test-id="UnitHeader-license"]"#).unwrap();

    if let Some(span_element) = document.select(&span_selector).next() {
        if let Some(a_element) = span_element.select(&a_selector).next() {
            return Some(
                a_element
                    .text()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_string(),
            );
        }
    }
    None
}

fn is_license_restrictive(
    license: &Option<String>,
    known_licenses: &HashMap<String, License>,
) -> bool {
    if license.as_deref() == Some("No License") {
        return true;
    }
    if let Some(license) = license {
        // println!("License: {}", license);
        // println!("Known Licenses: {:?}", known_licenses);
        if let Some(license_data) = known_licenses.get(license) {
            // println!("License Data: {:?}", license_data);
            const CONDITIONS: [&str; 2] = ["source-disclosure", "network-use-disclosure"];
            return CONDITIONS
                .iter()
                .any(|&condition| license_data.conditions.contains(&condition.to_string()));
        } else if RESTRICTIVE_LICENSES
            .iter()
            .any(|&restrictive_license| license.contains(restrictive_license))
        {
            return true;
        }
    }
    false
}

fn fetch_licenses_from_github() -> std::collections::HashMap<String, License> {
    let licenses_url =
        "https://raw.githubusercontent.com/github/choosealicense.com/gh-pages/_licenses/";
    let response = reqwest::blocking::get(licenses_url).expect("Failed to fetch licenses list");
    let content = response.text().expect("Failed to read response text");
    let mut licenses_map = std::collections::HashMap::new();
    for line in content.lines() {
        if line.ends_with(".txt") {
            let license_name = line.replace(".txt", "");
            let license_url = format!("{}{}", licenses_url, line);
            let license_content = reqwest::blocking::get(&license_url)
                .expect("Failed to fetch license content")
                .text()
                .expect("Failed to read license content");
            let license: License =
                serde_yaml::from_str(&license_content).expect("Failed to parse license content");
            licenses_map.insert(license_name, license);
        }
    }
    licenses_map
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockall::mock;
    use mockall::predicate::*;

    #[test]
    fn test_extract_license_from_html() {
        let html_content = r#"
            <html>
                <body>
                    <span class="go-Main-headerDetailItem" data-test-id="UnitHeader-licenses">
                        <a data-test-id="UnitHeader-license">MIT</a>
                    </span>
                </body>
            </html>
        "#;
        let license = extract_license_from_html(html_content);
        assert_eq!(license, Some("MIT".to_string()));
    }

    #[test]
    fn test_extract_license_from_html_no_license() {
        let html_content = r#"
            <html>
                <body>
                    <span class="go-Main-headerDetailItem" data-test-id="UnitHeader-licenses">
                    </span>
                </body>
            </html>
        "#;
        let license = extract_license_from_html(html_content);
        assert_eq!(license, None);
    }

    pub trait HttpClient {
        #[allow(dead_code)]
        fn get(&self, url: &str) -> Result<reqwest::blocking::Response, reqwest::Error>;
    }

    mock! {
        pub HttpClient {
            fn get(&self, url: &str) -> Result<reqwest::blocking::Response, reqwest::Error>;
        }
    }

    impl HttpClient for MockHttpClient {
        fn get(&self, url: &str) -> Result<reqwest::blocking::Response, reqwest::Error> {
            self.get(url)
        }
    }

    #[test]
    fn test_fetch_license_for_go_dependency() {
        let mut mock_http_client = MockHttpClient::new();

        mock_http_client
            .expect_get()
            .with(eq("https://pkg.go.dev/github.com/stretchr/testify"))
            .returning(|_| {
                let response = reqwest::blocking::Client::new()
                    .get("https://pkg.go.dev/github.com/stretchr/testify")
                    .send()
                    .unwrap();
                Ok(response)
            });

        let license = fetch_license_for_go_dependency("github.com/stretchr/testify", "v1.7.0");
        assert_eq!(license, "MIT");
    }

    #[test]
    fn test_fetch_license_for_python_dependency() {
        let mut mock_http_client = MockHttpClient::new();

        mock_http_client
            .expect_get()
            .with(eq("https://pypi.org/pypi/requests/2.25.1/json"))
            .returning(|_| {
                let response = reqwest::blocking::Client::new()
                    .get("https://pypi.org/pypi/requests/2.25.1/json")
                    .send()
                    .unwrap();
                Ok(response)
            });

        let license = fetch_license_for_python_dependency("requests", "2.25.1");
        assert_eq!(license, "Apache 2.0");
    }
}
