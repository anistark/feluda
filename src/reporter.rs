use crate::licenses::LicenseInfo;

pub fn generate_report(data: Vec<LicenseInfo>, json: bool, verbose: bool, strict: bool) {
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
        for info in filtered_data {
            if verbose {
                println!(
                    "Name: {}, Version: {}, License: {:?}, Restrictive: {}",
                    info.name, info.version, info.get_license(), info.is_restrictive
                );
            } else {
                println!("{}@{} - {:?}", info.name, info.version, info.get_license());
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
