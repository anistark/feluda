use cargo_metadata::Package;
use rayon::prelude::*;
use regex::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead};
use std::process::Command;
use toml::Value as TomlValue;

use crate::config;

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
        if let Some(deps) = self.dev_dependencies { all_dependencies.extend(deps) };
        if let Some(deps) = self.dependencies { all_dependencies.extend(deps) };
        all_dependencies
    }
}

/// Analyze the licenses of Python dependencies
pub fn analyze_python_licenses(package_file_path: &str) -> Vec<LicenseInfo> {
    let mut licenses = Vec::new();
    let known_licenses = fetch_licenses_from_github();

    // Check if it's a pyproject.toml file
    if package_file_path.ends_with("pyproject.toml") {
        let content =
            fs::read_to_string(package_file_path).expect("Failed to read pyproject.toml file");
        let config: TomlValue = toml::from_str(&content).expect("Failed to parse pyproject.toml");

        if let Some(project) = config.as_table().and_then(|t| t.get("project")) {
            if let Some(deps) = project
                .as_table()
                .and_then(|t| t.get("dependencies"))
                .and_then(|d| d.as_table())
            {
                for (name, version_value) in deps.iter() {
                    let version = match version_value.as_str() {
                        Some(v) => v.trim_matches('"').replace("^", "").replace("~", ""),
                        None => "latest".to_string(),
                    };
                    let license = Some(fetch_license_for_python_dependency(name, &version));
                    let is_restrictive = is_license_restrictive(&license, &known_licenses);

                    licenses.push(LicenseInfo {
                        name: name.to_string(),
                        version,
                        license,
                        is_restrictive,
                    });
                }
            }
        }
    } else {
        // Handle requirements.txt format
        let file = File::open(package_file_path).expect("Failed to open requirements.txt file");
        let reader = io::BufReader::new(file);

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

// Structure to hold license details for Go
#[derive(Debug)]
pub struct GoPackages {
    name: String,
    version: String,
}

/// Analyze the licenses of Go dependencies
/// TODO: The return should be Result<Vec<LicenseInfo>>
pub fn analyze_go_licenses(go_mod_path: &str) -> Vec<LicenseInfo> {
    let known_licenses = fetch_licenses_from_github();
    let content = fs::read_to_string(go_mod_path).expect("Failed to load file");
    let dependencies = get_go_dependencies(content);
    dependencies
        .par_iter()
        .map(|dependency| -> LicenseInfo {
            let name = dependency.name.clone();
            let version = dependency.version.clone();
            let license = Some(fetch_license_for_go_dependency(
                name.as_str(),
                version.as_str(),
            ));
            // println!("{}: {}", name, license.as_ref().unwrap());
            let is_restrictive = is_license_restrictive(&license, &known_licenses);
            LicenseInfo {
                name,
                version,
                license,
                is_restrictive,
            }
        })
        .collect()
}

pub fn get_go_dependencies(content_string: String) -> Vec<GoPackages> {
    let re =
        Regex::new(r"require\s*\(\s*((?:[\w./-]+\s+v[\d]+(?:\.\d+)*(?:-\S+)?\s*)+)\)").unwrap();
    let mut dependency = vec![];
    for cap in re.captures_iter(content_string.as_str()) {
        let dependency_block = &cap[1];
        let re_dependency = Regex::new(r"([\w./-]+)\s+(v[\d]+(?:\.\d+)*(?:-\S+)?)").unwrap();
        for dep_cap in re_dependency.captures_iter(dependency_block) {
            dependency.push(GoPackages {
                name: dep_cap[1].to_string(),
                version: dep_cap[2].to_string(),
            });
        }
    }
    dependency
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
    let config = match config::load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error loading configuration: {}", e);
            config::FeludaConfig::default()
        }
    };

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
        } else {
            return config
                .licenses
                .restrictive
                .iter()
                .any(|restrictive_license| license.contains(restrictive_license));
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
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_license_restrictive_with_default_config() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("GPL-3.0".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("MIT".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_license_restrictive_with_toml_config() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["CUSTOM-1.0"]"#,
            )
            .unwrap();

            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("CUSTOM-1.0".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("GPL-3.0".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_license_restrictive_with_env_config() {
        temp_env::with_vars(
            vec![("FELUDA_LICENSES_RESTRICTIVE", Some(r#"["ENV-LICENSE"]"#))],
            || {
                let dir = setup();
                std::env::set_current_dir(dir.path()).unwrap();

                let known_licenses = HashMap::new();
                assert!(is_license_restrictive(
                    &Some("ENV-LICENSE".to_string()),
                    &known_licenses
                ));
                assert!(!is_license_restrictive(
                    &Some("GPL-3.0".to_string()),
                    &known_licenses
                ));
            },
        );
    }

    #[test]
    fn test_env_overrides_toml() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", Some("[\"ENV-1.0\"]"), || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            fs::write(
                ".feluda.toml",
                r#"[licenses]
restrictive = ["TOML-1.0", "TOML-2.0"]"#,
            )
            .unwrap();

            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("ENV-1.0".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("TOML-1.0".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_license_restrictive_no_license() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            let known_licenses = HashMap::new();
            assert!(is_license_restrictive(
                &Some("No License".to_string()),
                &known_licenses
            ));
        });
    }

    #[test]
    fn test_license_restrictive_with_known_licenses() {
        temp_env::with_var("FELUDA_LICENSES_RESTRICTIVE", None::<&str>, || {
            let dir = setup();
            std::env::set_current_dir(dir.path()).unwrap();

            let mut known_licenses = HashMap::new();
            known_licenses.insert(
                "TEST-LICENSE".to_string(),
                License {
                    title: "Test License".to_string(),
                    spdx_id: "TEST-LICENSE".to_string(),
                    permissions: vec![],
                    conditions: vec!["source-disclosure".to_string()],
                    limitations: vec![],
                },
            );

            assert!(is_license_restrictive(
                &Some("TEST-LICENSE".to_string()),
                &known_licenses
            ));
            assert!(!is_license_restrictive(
                &Some("OTHER-LICENSE".to_string()),
                &known_licenses
            ));
        });
    }

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

    #[test]
    fn test_analyze_python_licenses_pyproject_toml() {
        let temp_dir = setup();
        let pyproject_toml_path = temp_dir.path().join("pyproject.toml");
        fs::write(
            &pyproject_toml_path,
            r#"[project]
name = "test-project"
version = "0.1.0"

[project.dependencies]
requests = "^2.31.0"
flask = "~2.0.0"
"#,
        )
        .unwrap();

        let result = analyze_python_licenses(pyproject_toml_path.to_str().unwrap());
        assert!(!result.is_empty());
        assert!(result.iter().any(|info| info.name == "requests"));
        assert!(result.iter().any(|info| info.name == "flask"));
    }
}
