use cargo_metadata::Package;
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;

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
