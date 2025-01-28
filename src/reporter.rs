use crate::licenses::LicenseInfo;
use std::collections::HashMap;

pub fn generate_report(data: Vec<LicenseInfo>, json: bool, verbose: bool, strict: bool) {
    let total_packages = data.len();
    let filtered_data: Vec<LicenseInfo> = if strict {
        data.into_iter().filter(|info| info.is_restrictive).collect()
    } else {
        data
    };

    if filtered_data.is_empty() {
        println!("\n🎉 All dependencies passed the license check! No restrictive licenses found.\n");
        return;
    }

    if json {
        let json_output = serde_json::to_string_pretty(&filtered_data).expect("Failed to serialize data");
        println!("{}", json_output);
    } else {
        let mut restrictive_licenses: Vec<LicenseInfo> = Vec::new();
        let mut license_count: HashMap<Option<String>, usize> = HashMap::new();
        for info in filtered_data {
            if verbose {
                println!(
                    "Name: {}, Version: {}, License: {:?}, Restrictive: {}",
                    info.name, info.version, info.get_license(), info.is_restrictive
                );
            }
            // else {
            //     println!("{}@{} - {:?}", info.name, info.version, info.get_license());
            // }
            if info.is_restrictive {
                // Add to a separate array or handle as needed
                restrictive_licenses.push(info);
            } else {
                *license_count.entry(info.license.clone()).or_insert(0) += 1;
            }
        }

        println!("{:<49} {:<5}", "License Type", "Dependencies");
        println!("{:<53} {:<5}", "---------------------", "------------");
        for (license, count) in license_count {
            println!("{:<58} {:<5}", license.unwrap_or_else(|| "Unknown".to_string()), count);
        }
        println!("\nTotal dependencies scanned: {}", total_packages);
        if restrictive_licenses.is_empty() {
            println!("\n✅ No restrictive licenses found! 🎉\n");
        } else {
            println!("\n{:<49} {:<5}", "Restrictive License Type", "Dependencies");
            println!("{:<53} {:<5}", "---------------------", "------------");
            for info in restrictive_licenses {
            println!("{:<58} {:<5}", info.license.unwrap_or_else(|| "Unknown".to_string()), 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_report_empty_data() {
        let data = vec![];
        generate_report(data, false, false, false);
        // Expected output: 🎉 All dependencies passed the license check! No restrictive licenses found.
    }

    #[test]
    fn test_generate_report_non_strict() {
        let data = vec![
            LicenseInfo {
                name: "crate1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
            },
            LicenseInfo {
                name: "crate2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("GPL".to_string()),
                is_restrictive: true,
            },
        ];
        generate_report(data, false, false, false);
        // Expected output: crate1@1.0.0 - Some("MIT")
        //                  crate2@2.0.0 - Some("GPL")
    }

    #[test]
    fn test_generate_report_strict() {
        let data = vec![
            LicenseInfo {
                name: "crate1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
            },
            LicenseInfo {
                name: "crate2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("GPL".to_string()),
                is_restrictive: true,
            },
        ];
        generate_report(data, false, false, true);
        // Expected output: crate2@2.0.0 - Some("GPL")
    }

    #[test]
    fn test_generate_report_json() {
        let data = vec![
            LicenseInfo {
                name: "crate1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
            },
            LicenseInfo {
                name: "crate2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("GPL".to_string()),
                is_restrictive: true,
            },
        ];
        generate_report(data, true, false, false);
        // Expected output: JSON formatted string of the data
    }

    #[test]
    fn test_generate_report_verbose() {
        let data = vec![
            LicenseInfo {
                name: "crate1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
            },
            LicenseInfo {
                name: "crate2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("GPL".to_string()),
                is_restrictive: true,
            },
        ];
        generate_report(data, false, true, false);
        // Expected output: 
        // Name: crate1, Version: 1.0.0, License: Some("MIT"), Restrictive: false
        // Name: crate2, Version: 2.0.0, License: Some("GPL"), Restrictive: true
    }
}
