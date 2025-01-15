use cargo_metadata::MetadataCommand;
use std::path::Path;

use crate::licenses::{analyze_js_licenses, analyze_rust_licenses, analyze_go_licenses, LicenseInfo};

// Detect what type of project is this
fn detect_project_type(args_path: &str) -> Option<&'static str> {
    let project_path = std::fs::canonicalize(args_path)
        .unwrap_or_else(|_| panic!("❌ Error: Invalid path '{}'", args_path));

    if Path::new(&project_path).join("Cargo.toml").exists() {
        println!("🦀");
        Some("rust")
    } else if Path::new(&project_path).join("package.json").exists() {
        println!("⬢");
        Some("node")
    } else if Path::new(&project_path).join("go.mod").exists() {
        println!("🐹");
        Some("go")
    } else {
        None
    }
}

pub fn parse_dependencies(project_path: &str) -> Vec<LicenseInfo> {
    match detect_project_type(project_path) {
        Some("rust") => {
            let project_path = Path::new(project_path).join("Cargo.toml");
            let metadata = MetadataCommand::new()
                .manifest_path(Path::new(&project_path))
                .exec()
                .expect("Failed to fetch cargo metadata");

            let analyzed_data = analyze_rust_licenses(metadata.packages);

            analyzed_data
        }
        Some("node") => {
            let project_path = Path::new(project_path).join("package.json");
            let analyzed_data = analyze_js_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            );

            analyzed_data
        }
        Some("go") => {
            let project_path = Path::new(project_path).join("go.mod");
            let analyzed_data = analyze_go_licenses(
                project_path
                    .to_str()
                    .expect("Failed to convert path to string"),
            );
            analyzed_data
        }
        Some(_) => {
            eprintln!("⚠️ Unsupported project type detected.");
            std::process::exit(1);
        }
        None => {
            eprintln!("❌ Unable to detect project type.");
            std::process::exit(1);
        }
    }
}
