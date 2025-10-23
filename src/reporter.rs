use crate::cli::{CiFormat, OsiFilter};
use crate::debug::{log, log_debug, log_error, LogLevel};
use crate::licenses::{LicenseCompatibility, LicenseInfo, OsiStatus};
use colored::*;
use std::collections::HashMap;
use std::fs;

// ReportConfig struct
#[derive(Debug)]
pub struct ReportConfig {
    json: bool,
    yaml: bool,
    verbose: bool,
    restrictive: bool,
    incompatible: bool,
    ci_format: Option<CiFormat>,
    output_file: Option<String>,
    project_license: Option<String>,
    gist: bool,
    osi: Option<OsiFilter>,
}

impl ReportConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        json: bool,
        yaml: bool,
        verbose: bool,
        restrictive: bool,
        incompatible: bool,
        ci_format: Option<CiFormat>,
        output_file: Option<String>,
        project_license: Option<String>,
        gist: bool,
        osi: Option<OsiFilter>,
    ) -> Self {
        Self {
            json,
            yaml,
            verbose,
            restrictive,
            incompatible,
            ci_format,
            output_file,
            project_license,
            gist,
            osi,
        }
    }
}

struct TableFormatter {
    column_widths: Vec<usize>,
    headers: Vec<String>,
}

impl TableFormatter {
    fn new(headers: Vec<String>) -> Self {
        let column_widths = headers.iter().map(|h| h.len()).collect();
        Self {
            column_widths,
            headers,
        }
    }

    fn add_row(&mut self, row: &[String]) {
        for (i, item) in row.iter().enumerate() {
            if i < self.column_widths.len() {
                self.column_widths[i] = self.column_widths[i].max(item.len());
            }
        }
    }

    fn render_header(&self) -> String {
        let header_row = self
            .headers
            .iter()
            .enumerate()
            .map(|(i, header)| format!("{:width$}", header, width = self.column_widths[i]))
            .collect::<Vec<_>>()
            .join(" │ ");

        let total_width =
            self.column_widths.iter().sum::<usize>() + (3 * self.column_widths.len()) - 1;

        format!(
            "┌{}┐\n│ {} │\n├{}┤",
            "─".repeat(total_width),
            header_row.bold().blue(),
            "─".repeat(total_width)
        )
    }

    fn render_row(&self, row: &[String], is_problematic: bool) -> String {
        let formatted_row = row
            .iter()
            .enumerate()
            .map(|(i, item)| {
                if i < self.column_widths.len() {
                    format!("{:width$}", item, width = self.column_widths[i])
                } else {
                    item.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" │ ");

        if is_problematic {
            format!("│ {} │", formatted_row.red().bold())
        } else {
            format!("│ {} │", formatted_row.green())
        }
    }

    fn render_footer(&self) -> String {
        let footer_width =
            self.column_widths.iter().sum::<usize>() + (3 * self.column_widths.len()) - 1;
        format!("└{}┘", "─".repeat(footer_width))
    }
}

pub fn generate_report(data: Vec<LicenseInfo>, config: ReportConfig) -> (bool, bool) {
    log(
        LogLevel::Info,
        &format!("Generating report with config: {config:?}"),
    );

    let total_packages = data.len();
    log(
        LogLevel::Info,
        &format!("Total packages to analyze: {total_packages}"),
    );

    let has_restrictive = data.iter().any(|info| *info.is_restrictive());
    let has_incompatible = data
        .iter()
        .any(|info| info.compatibility == LicenseCompatibility::Incompatible);

    log(
        LogLevel::Info,
        &format!("Has restrictive licenses: {has_restrictive}"),
    );

    log(
        LogLevel::Info,
        &format!("Has incompatible licenses: {has_incompatible}"),
    );

    if config.gist {
        log(LogLevel::Info, "Generating gist summary");
        print_gist_summary(&data, total_packages, config.project_license.as_deref());
        return (has_restrictive, has_incompatible);
    }

    // Filter data if in restrictive or/and incompatible mode to show only restrictive or/and incompatible licenses
    let mut filtered_data: Vec<LicenseInfo> = if config.restrictive || config.incompatible {
        log(
            LogLevel::Info,
            "Restrictive or/and incompatible mode enabled, filtering restrictive or/and incompatible licenses only",
        );
        data.into_iter()
            .filter(|info| {
                (config.restrictive && *info.is_restrictive())
                    || (config.incompatible
                        && (info.compatibility == LicenseCompatibility::Incompatible))
            })
            .collect()
    } else {
        data
    };

    // Apply OSI filtering
    if let Some(osi_filter) = &config.osi {
        let before_count = filtered_data.len();
        match osi_filter {
            OsiFilter::Approved => {
                filtered_data.retain(|info| info.osi_status == OsiStatus::Approved);
                log(
                    LogLevel::Info,
                    &format!(
                        "Applied OSI approved filter: {} of {} dependencies",
                        filtered_data.len(),
                        before_count
                    ),
                );
            }
            OsiFilter::NotApproved => {
                filtered_data.retain(|info| info.osi_status == OsiStatus::NotApproved);
                log(
                    LogLevel::Info,
                    &format!(
                        "Applied OSI not-approved filter: {} of {} dependencies",
                        filtered_data.len(),
                        before_count
                    ),
                );
            }
            OsiFilter::Unknown => {
                filtered_data.retain(|info| info.osi_status == OsiStatus::Unknown);
                log(
                    LogLevel::Info,
                    &format!(
                        "Applied OSI unknown filter: {} of {} dependencies",
                        filtered_data.len(),
                        before_count
                    ),
                );
            }
        }
    }

    log(
        LogLevel::Info,
        &format!("Filtered packages count: {}", filtered_data.len()),
    );
    log_debug("Filtered license data", &filtered_data);

    if filtered_data.is_empty() {
        println!(
            "\n{}\n",
            "🎉 All dependencies passed the license check! No restrictive or incompatible licenses found."
                .green()
                .bold()
        );
        return (false, false);
    }

    if let Some(format) = config.ci_format {
        match format {
            CiFormat::Github => output_github_format(
                &filtered_data,
                config.output_file.as_deref(),
                config.project_license.as_deref(),
            ),
            CiFormat::Jenkins => output_jenkins_format(
                &filtered_data,
                config.output_file.as_deref(),
                config.project_license.as_deref(),
            ),
        }
    } else if config.json {
        // JSON output
        log(LogLevel::Info, "Generating JSON output");
        match serde_json::to_string_pretty(&filtered_data) {
            Ok(json_output) => println!("{json_output}"),
            Err(err) => {
                log_error("Failed to serialize data to JSON", &err);
                println!("Error: Failed to generate JSON output");
            }
        }
    } else if config.yaml {
        // YAML output
        log(LogLevel::Info, "Generating YAML output");
        match serde_yaml::to_string(&filtered_data) {
            Ok(yaml_output) => println!("{yaml_output}"),
            Err(err) => {
                log_error("Failed to serialize data to YAML", &err);
                println!("Error: Failed to generate YAML output");
            }
        }
    } else if config.verbose {
        log(LogLevel::Info, "Generating verbose table");
        print_verbose_table(
            &filtered_data,
            config.restrictive,
            config.project_license.as_deref(),
        );
    } else {
        log(LogLevel::Info, "Generating summary table");
        print_summary_table(
            &filtered_data,
            total_packages,
            config.restrictive,
            config.incompatible,
            config.project_license.as_deref(),
        );
    }

    (has_restrictive, has_incompatible)
}

fn print_verbose_table(
    license_info: &[LicenseInfo],
    restrictive: bool,
    project_license: Option<&str>,
) {
    log(LogLevel::Info, "Printing verbose table");

    let mut headers = vec![
        "Name".to_string(),
        "Version".to_string(),
        "License".to_string(),
        "Restrictive".to_string(),
    ];

    // Add compatibility column if project license is available
    if project_license.is_some() {
        headers.push("Compatibility".to_string());
    }

    // Always add OSI status column in verbose mode
    headers.push("OSI Status".to_string());

    let mut formatter = TableFormatter::new(headers);

    let rows: Vec<_> = license_info
        .iter()
        .map(|info| {
            let mut row = vec![
                info.name().to_string(),
                info.version().to_string(),
                info.get_license(),
                info.is_restrictive().to_string(),
            ];

            // Add compatibility if project license is available
            if project_license.is_some() {
                row.push(format!("{:?}", info.compatibility));
            }

            // Always add OSI status in verbose mode
            row.push(info.osi_status().to_string());

            row
        })
        .collect();

    log_debug("Table rows prepared", &rows);

    for row in &rows {
        formatter.add_row(row);
    }

    println!("\n{}", formatter.render_header());

    for (i, row) in rows.iter().enumerate() {
        let is_restrictive = *license_info[i].is_restrictive();
        let is_incompatible =
            *license_info[i].compatibility() == LicenseCompatibility::Incompatible;

        println!(
            "{}",
            formatter.render_row(row, is_restrictive || is_incompatible)
        );
    }

    println!("{}\n", formatter.render_footer());

    if !restrictive {
        print_summary_footer(license_info, project_license);
    }
}

fn print_summary_table(
    license_info: &[LicenseInfo],
    total_packages: usize,
    restrictive: bool,
    incompatible: bool,
    project_license: Option<&str>,
) {
    log(LogLevel::Info, "Printing summary table");

    // Print project license if available
    if let Some(license) = project_license {
        println!(
            "\n{} {}",
            "📄".bold(),
            format!("Project License: {license}").bold()
        );
    }

    let mut license_count: HashMap<String, Vec<String>> = HashMap::new();
    let mut restrictive_licenses: Vec<&LicenseInfo> = Vec::new();
    let mut incompatible_licenses: Vec<&LicenseInfo> = Vec::new();

    for info in license_info {
        let license = info.get_license();

        if *info.is_restrictive() {
            restrictive_licenses.push(info);
        } else {
            license_count
                .entry(license)
                .or_default()
                .push(info.name().to_string());
        }

        if info.compatibility == LicenseCompatibility::Incompatible {
            incompatible_licenses.push(info);
        }
    }

    log(
        LogLevel::Info,
        &format!("Found {} permissive license types", license_count.len()),
    );
    log(
        LogLevel::Info,
        &format!(
            "Found {} packages with restrictive licenses",
            restrictive_licenses.len()
        ),
    );
    log(
        LogLevel::Info,
        &format!(
            "Found {} packages with incompatible licenses",
            incompatible_licenses.len()
        ),
    );

    if restrictive || incompatible {
        if restrictive && !restrictive_licenses.is_empty() {
            log(
                LogLevel::Info,
                "Restrictive mode enabled, showing only restrictive licenses",
            );
            print_restrictive_licenses_table(&restrictive_licenses);
        }
        if incompatible && project_license.is_some() && !incompatible_licenses.is_empty() {
            if let Some(license) = project_license {
                print_incompatible_licenses_table(&incompatible_licenses, license);
            }
        }
        return;
    }

    // License summary
    let headers = vec!["License Type".to_string(), "Count".to_string()];

    let mut formatter = TableFormatter::new(headers);

    let mut rows: Vec<Vec<String>> = license_count
        .iter()
        .map(|(license, deps)| vec![license.clone(), deps.len().to_string()])
        .collect();

    for row in &rows {
        formatter.add_row(row);
    }

    println!(
        "\n{} {}\n",
        "🔍".bold(),
        "License Summary".bold().underline()
    );

    println!("{}", formatter.render_header());

    rows.sort_by(|a, b| a[0].cmp(&b[0]));

    for row in &rows {
        println!("{}", formatter.render_row(row, true));
    }

    println!("{}", formatter.render_footer());

    println!(
        "\n{} {}",
        "📦".bold(),
        format!("Total dependencies scanned: {total_packages}").bold()
    );

    if !restrictive_licenses.is_empty() {
        print_restrictive_licenses_table(&restrictive_licenses);
    } else {
        println!(
            "\n{}\n",
            "✅ No restrictive licenses found! 🎉".green().bold()
        );
    }

    // Print incompatible licenses if project license is available
    if project_license.is_some() && !incompatible_licenses.is_empty() {
        if let Some(license) = project_license {
            print_incompatible_licenses_table(&incompatible_licenses, license);
        }
    } else if project_license.is_some() {
        println!(
            "\n{}\n",
            "✅ No incompatible licenses found! 🎉".green().bold()
        );
    }
}

fn print_restrictive_licenses_table(restrictive_licenses: &[&LicenseInfo]) {
    log(
        LogLevel::Info,
        &format!(
            "Printing table for {} restrictive licenses",
            restrictive_licenses.len()
        ),
    );

    println!(
        "\n{} {}\n",
        "⚠️".bold(),
        "Warning: Restrictive licenses found!".yellow().bold()
    );

    let headers = vec![
        "Package".to_string(),
        "Version".to_string(),
        "License".to_string(),
    ];

    let mut formatter = TableFormatter::new(headers);

    let rows: Vec<_> = restrictive_licenses
        .iter()
        .map(|info| {
            vec![
                info.name().to_string(),
                info.version().to_string(),
                info.get_license(),
            ]
        })
        .collect();

    for row in &rows {
        formatter.add_row(row);
    }

    println!("{}", formatter.render_header());

    for row in &rows {
        println!("{}", formatter.render_row(row, false));
    }

    println!("{}\n", formatter.render_footer());
}

fn print_incompatible_licenses_table(
    incompatible_licenses: &[&LicenseInfo],
    project_license: &str,
) {
    log(
        LogLevel::Info,
        &format!(
            "Printing table for {} incompatible licenses",
            incompatible_licenses.len()
        ),
    );

    println!(
        "\n{} {}\n",
        "❌".bold(),
        format!("Warning: Licenses incompatible with {project_license} found!")
            .red()
            .bold()
    );

    let headers = vec![
        "Package".to_string(),
        "Version".to_string(),
        "License".to_string(),
    ];

    let mut formatter = TableFormatter::new(headers);

    let rows: Vec<_> = incompatible_licenses
        .iter()
        .map(|info| {
            vec![
                info.name().to_string(),
                info.version().to_string(),
                info.get_license(),
            ]
        })
        .collect();

    for row in &rows {
        formatter.add_row(row);
    }

    println!("{}", formatter.render_header());

    for row in &rows {
        println!("{}", formatter.render_row(row, false));
    }

    println!("{}\n", formatter.render_footer());
}

fn print_summary_footer(license_info: &[LicenseInfo], project_license: Option<&str>) {
    log(LogLevel::Info, "Printing summary footer");

    let total = license_info.len();
    let restrictive_count = license_info.iter().filter(|i| *i.is_restrictive()).count();
    let permissive_count = total - restrictive_count;

    // Calculate compatibility counts if project license is available
    let (compatible_count, incompatible_count, unknown_count) = if project_license.is_some() {
        (
            license_info
                .iter()
                .filter(|i| i.compatibility == LicenseCompatibility::Compatible)
                .count(),
            license_info
                .iter()
                .filter(|i| i.compatibility == LicenseCompatibility::Incompatible)
                .count(),
            license_info
                .iter()
                .filter(|i| i.compatibility == LicenseCompatibility::Unknown)
                .count(),
        )
    } else {
        (0, 0, 0)
    };

    println!("{}", "🔍 License Summary:".bold());
    println!(
        "  • {} {}",
        permissive_count.to_string().green().bold(),
        "permissive licenses".green()
    );
    println!(
        "  • {} {}",
        restrictive_count.to_string().yellow().bold(),
        "restrictive licenses".yellow()
    );

    // Print compatibility info if project license is available
    if project_license.is_some() {
        println!(
            "  • {} {}",
            compatible_count.to_string().green().bold(),
            "compatible licenses".green()
        );
        println!(
            "  • {} {}",
            incompatible_count.to_string().red().bold(),
            "incompatible licenses".red()
        );
        println!(
            "  • {} {}",
            unknown_count.to_string().blue().bold(),
            "unknown compatibility".blue()
        );
    }

    println!("  • {total} total dependencies");

    if restrictive_count > 0 {
        println!("\n{} {}: Review these dependencies for compliance with your project's licensing requirements.",
            "⚠️".yellow().bold(),
            "Recommendation".yellow().bold()
        );
    } else {
        println!(
            "\n{} {}: All dependencies have permissive licenses compatible with most projects.",
            "✅".green().bold(),
            "Status".green().bold()
        );
    }

    // Add compatibility recommendation if project license is available
    if project_license.is_some() && incompatible_count > 0 {
        println!("\n{} {}: Some dependencies have licenses that may be incompatible with your project's {} license. Review for legal compliance.",
            "❌".red().bold(),
            "Warning".red().bold(),
            project_license.unwrap()
        );
    }

    println!();
}

fn output_github_format(
    license_info: &[LicenseInfo],
    output_path: Option<&str>,
    project_license: Option<&str>,
) {
    log(
        LogLevel::Info,
        "Generating GitHub Actions compatible output",
    );

    // GitHub Actions workflow commands format
    let mut output = String::new();

    // Add project license info if available
    if let Some(license) = project_license {
        output.push_str(&format!(
            "::notice title=Project License::Project is using {license} license\n"
        ));
    }

    // GitHub Actions workflow commands format for restrictive licenses
    for info in license_info {
        if *info.is_restrictive() {
            let warning = format!(
                "::warning title=Restrictive License::Dependency '{}@{}' has restrictive license: {}\n",
                info.name(),
                info.version(),
                info.get_license()
            );
            output.push_str(&warning);

            log(
                LogLevel::Info,
                &format!("Added warning for restrictive license: {}", info.name()),
            );
        }

        // Add incompatible license warnings if project license is available
        if project_license.is_some() && info.compatibility == LicenseCompatibility::Incompatible {
            let warning = format!(
                "::error title=Incompatible License::Dependency '{}@{}' has license {} which may be incompatible with project license {}\n",
                info.name(),
                info.version(),
                info.get_license(),
                project_license.unwrap()
            );
            output.push_str(&warning);

            log(
                LogLevel::Info,
                &format!("Added error for incompatible license: {}", info.name()),
            );
        }
    }

    let restrictive_count = license_info.iter().filter(|i| *i.is_restrictive()).count();
    let incompatible_count = if project_license.is_some() {
        license_info
            .iter()
            .filter(|i| i.compatibility == LicenseCompatibility::Incompatible)
            .count()
    } else {
        0
    };

    let summary = if project_license.is_some() {
        format!(
            "::notice title=License Check Summary::Found {} dependencies with restrictive licenses and {} dependencies with incompatible licenses out of {} total\n",
            restrictive_count,
            incompatible_count,
            license_info.len()
        )
    } else {
        format!(
            "::notice title=License Check Summary::Found {} dependencies with restrictive licenses out of {} total\n",
            restrictive_count,
            license_info.len()
        )
    };

    output.push_str(&summary);

    log(
        LogLevel::Info,
        &format!(
            "Added summary: {} restrictive and {} incompatible out of {}",
            restrictive_count,
            incompatible_count,
            license_info.len()
        ),
    );

    // Output to file or stdout
    if let Some(path) = output_path {
        log(
            LogLevel::Info,
            &format!("Writing GitHub Actions output to file: {path}"),
        );

        match fs::write(path, &output) {
            Ok(_) => println!("GitHub Actions output written to: {path}"),
            Err(err) => {
                log_error(
                    &format!("Failed to write GitHub Actions output file: {path}"),
                    &err,
                );
                println!("Error: Failed to write GitHub Actions output file");
                println!("{output}");
            }
        }
    } else {
        log(LogLevel::Info, "Writing GitHub Actions output to stdout");
        print!("{output}");
    }
}

fn output_jenkins_format(
    license_info: &[LicenseInfo],
    output_path: Option<&str>,
    project_license: Option<&str>,
) {
    log(
        LogLevel::Info,
        "Generating Jenkins compatible output (JUnit XML)",
    );

    // Jenkins compatible output (JUnit XML format)
    let mut test_cases = Vec::new();

    // Add project license info if available
    if let Some(license) = project_license {
        test_cases.push(format!(
            r#"    <testcase classname="feluda.project" name="project_license" time="0">
        <system-out>Project is using {license} license</system-out>
    </testcase>"#
        ));
    }

    for info in license_info {
        let test_case_name = format!("{}-{}", info.name(), info.version());
        log(
            LogLevel::Info,
            &format!("Processing test case: {test_case_name}"),
        );

        let mut failures = Vec::new();

        // Check for restrictive license
        if *info.is_restrictive() {
            failures.push(format!(
                r#"<failure message="Restrictive license found" type="restrictive">
            Dependency '{}@{}' has restrictive license: {}
        </failure>"#,
                info.name(),
                info.version(),
                info.get_license()
            ));

            log(
                LogLevel::Info,
                &format!(
                    "Added failing test case for restrictive license: {}",
                    info.name()
                ),
            );
        }

        // Check for incompatible license if project license is available
        if project_license.is_some() && info.compatibility == LicenseCompatibility::Incompatible {
            failures.push(format!(
                r#"<failure message="Incompatible license found" type="incompatible">
            Dependency '{}@{}' has license {} which may be incompatible with project license {}
        </failure>"#,
                info.name(),
                info.version(),
                info.get_license(),
                project_license.unwrap()
            ));

            log(
                LogLevel::Info,
                &format!(
                    "Added failing test case for incompatible license: {}",
                    info.name()
                ),
            );
        }

        if failures.is_empty() {
            test_cases.push(format!(
                r#"    <testcase classname="feluda.licenses" name="{test_case_name}" time="0" />"#
            ));
        } else {
            test_cases.push(format!(
                r#"    <testcase classname="feluda.licenses" name="{}" time="0">
{}
    </testcase>"#,
                test_case_name,
                failures.join("\n")
            ));
        }
    }

    let restrictive_count = license_info.iter().filter(|i| *i.is_restrictive()).count();
    let incompatible_count = if project_license.is_some() {
        license_info
            .iter()
            .filter(|i| i.compatibility == LicenseCompatibility::Incompatible)
            .count()
    } else {
        0
    };

    let failure_count = restrictive_count + incompatible_count;

    log(
        LogLevel::Info,
        &format!(
            "Total test cases: {}, failures: {}",
            license_info.len(),
            failure_count
        ),
    );

    let junit_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="Feluda License Check" tests="{}" failures="{}" errors="0" skipped="0">
{}
  </testsuite>
</testsuites>"#,
        license_info.len() + (if project_license.is_some() { 1 } else { 0 }),
        failure_count,
        test_cases.join("\n")
    );

    // Output to file or stdout
    if let Some(path) = output_path {
        log(
            LogLevel::Info,
            &format!("Writing Jenkins JUnit XML to file: {path}"),
        );

        match fs::write(path, &junit_xml) {
            Ok(_) => println!("Jenkins JUnit XML output written to: {path}"),
            Err(err) => {
                log_error(
                    &format!("Failed to write Jenkins output file: {path}"),
                    &err,
                );
                println!("Error: Failed to write Jenkins JUnit XML output file");
                println!("{junit_xml}"); // Fallback to stdout
            }
        }
    } else {
        log(LogLevel::Info, "Writing Jenkins JUnit XML to stdout");
        println!("{junit_xml}");
    }
}

// Add gist report function to reporter.rs
fn print_gist_summary(
    license_info: &[LicenseInfo],
    total_packages: usize,
    project_license: Option<&str>,
) {
    use colored::*;

    let restrictive_count = license_info.iter().filter(|i| *i.is_restrictive()).count();
    let incompatible_count = license_info
        .iter()
        .filter(|i| i.compatibility == LicenseCompatibility::Incompatible)
        .count();

    let project_license_display = project_license.unwrap_or("Not detected");

    println!("\n{}", "🦀 FELUDA GIST".bold().cyan());
    println!("{}", "━".repeat(50).cyan());

    println!(
        "│ {:30} │ {}",
        "Project License".bold(),
        project_license_display.cyan()
    );
    println!(
        "│ {:30} │ {}",
        "Total Dependencies Scanned".bold(),
        total_packages.to_string().cyan()
    );

    println!("{}", "━".repeat(50).cyan());

    let restrictive_status = if restrictive_count > 0 {
        format!(
            "{} {}",
            "⚠️".yellow(),
            restrictive_count.to_string().yellow().bold()
        )
    } else {
        format!("{} {}", "✅".green(), "0".green().bold())
    };

    let incompatible_status = if project_license.is_some() {
        if incompatible_count > 0 {
            format!(
                "{} {}",
                "❌".red(),
                incompatible_count.to_string().red().bold()
            )
        } else {
            format!("{} {}", "✅".green(), "0".green().bold())
        }
    } else {
        format!("{} {}", "❓".blue(), "N/A".blue())
    };

    println!(
        "│ {:30} │ {}",
        "Restrictive dependencies".bold(),
        restrictive_status
    );
    println!(
        "│ {:30} │ {}",
        "Incompatible dependencies".bold(),
        incompatible_status
    );

    println!("{}", "━".repeat(50).cyan());

    let overall_status = if restrictive_count > 0 || incompatible_count > 0 {
        format!("{} {}", "⚠️".yellow(), "NEEDS ATTENTION".yellow().bold())
    } else {
        format!("{} {}", "✨".green(), "ALL GOOD".green().bold())
    };

    println!("│ {:30} │ {}", "Recommendation".bold(), overall_status);

    println!("{}\n", "━".repeat(50).cyan());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::licenses::LicenseCompatibility;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    fn get_test_data() -> Vec<LicenseInfo> {
        vec![
            LicenseInfo {
                name: "crate1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "crate2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "crate3".to_string(),
                version: "3.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "crate4".to_string(),
                version: "4.0.0".to_string(),
                license: Some("Unknown".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::OsiStatus::Unknown,
            },
        ]
    }

    fn get_test_data_with_unknown_compatibility() -> Vec<LicenseInfo> {
        vec![
            LicenseInfo {
                name: "crate1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "crate2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ]
    }

    #[test]
    fn test_generate_report_empty_data() {
        let data = vec![];
        let config = ReportConfig::new(
            false, false, false, false, false, None, None, None, false, None,
        );
        let result = generate_report(data, config);
        assert_eq!(result, (false, false)); // No restrictive or incompatible licenses
    }

    #[test]
    fn test_generate_report_non_strict() {
        let data = get_test_data();
        let config = ReportConfig::new(
            false,
            false,
            false,
            false,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let result = generate_report(data, config);
        assert_eq!(result, (true, true)); // Has both restrictive and incompatible licenses
    }

    #[test]
    fn test_generate_report_strict() {
        let data = get_test_data();
        let config = ReportConfig::new(
            false,
            false,
            false,
            true,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let result = generate_report(data, config);
        assert_eq!(result, (true, true)); // In strict mode, still has both restrictive and incompatible
    }

    #[test]
    fn test_generate_report_json() {
        let data = get_test_data();
        let config = ReportConfig::new(
            true,
            false,
            false,
            false,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let result = generate_report(data, config);
        assert_eq!(result, (true, true));
    }

    #[test]
    fn test_generate_report_yaml() {
        let data = get_test_data();
        let config = ReportConfig::new(
            false,
            true,
            false,
            false,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let result = generate_report(data, config);
        assert_eq!(result, (true, true));
    }

    #[test]
    fn test_generate_report_verbose() {
        let data = get_test_data();
        let config = ReportConfig::new(
            false,
            false,
            true,
            false,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let result = generate_report(data, config);
        assert_eq!(result, (true, true));
    }

    #[test]
    fn test_generate_report_no_project_license() {
        let data = get_test_data_with_unknown_compatibility();
        let config = ReportConfig::new(
            false, false, false, false, false, None, None, None, false, None,
        );
        let result = generate_report(data, config);
        assert_eq!(result, (true, false)); // Has restrictive but no incompatible since no project license
    }

    #[test]
    fn test_github_output_format() {
        let data = get_test_data();
        let temp_dir = setup();
        let output_path = temp_dir.path().join("github_output.txt");
        let config = ReportConfig::new(
            false,
            false,
            false,
            false,
            false,
            Some(CiFormat::Github),
            Some(output_path.to_str().unwrap().to_string()),
            Some("MIT".to_string()),
            false,
            None,
        );

        let result = generate_report(data, config);
        assert_eq!(result, (true, true));

        let content = match fs::read_to_string(&output_path) {
            Ok(content) => content,
            Err(err) => {
                panic!("Failed to read output file: {err}");
            }
        };

        assert!(content.contains("::warning title=Restrictive License::"));
        assert!(content.contains("::error title=Incompatible License::"));
        assert!(content.contains("::notice title=Project License::"));
        assert!(content.contains("::notice title=License Check Summary::"));
    }

    #[test]
    fn test_jenkins_output_format() {
        let data = get_test_data();
        let temp_dir = setup();
        let output_path = temp_dir.path().join("jenkins_output.xml");
        let config = ReportConfig::new(
            false,
            false,
            false,
            false,
            false,
            Some(CiFormat::Jenkins),
            Some(output_path.to_str().unwrap().to_string()),
            Some("MIT".to_string()),
            false,
            None,
        );

        let result = generate_report(data, config);
        assert_eq!(result, (true, true));

        let content = match fs::read_to_string(&output_path) {
            Ok(content) => content,
            Err(err) => {
                panic!("Failed to read output file: {err}");
            }
        };

        assert!(content.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(content.contains("<testsuites>"));
        assert!(content.contains("<failure message=\"Restrictive license found\""));
        assert!(content.contains("<failure message=\"Incompatible license found\""));
        assert!(content.contains("Project is using MIT license"));
    }

    #[test]
    fn test_jenkins_output_format_no_project_license() {
        let data = get_test_data_with_unknown_compatibility();
        let temp_dir = setup();
        let output_path = temp_dir.path().join("jenkins_output.xml");
        let config = ReportConfig::new(
            false,
            false,
            false,
            false,
            false,
            Some(CiFormat::Jenkins),
            Some(output_path.to_str().unwrap().to_string()),
            None,
            false,
            None,
        );

        let result = generate_report(data, config);
        assert_eq!(result, (true, false)); // Has restrictive but no incompatible

        let content = match fs::read_to_string(&output_path) {
            Ok(content) => content,
            Err(err) => {
                panic!("Failed to read output file: {err}");
            }
        };

        assert!(content.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(content.contains("<testsuites>"));
        assert!(content.contains("<failure message=\"Restrictive license found\""));
        assert!(!content.contains("<failure message=\"Incompatible license found\""));
        assert!(!content.contains("Project is using"));
    }

    #[test]
    fn test_table_formatter() {
        let headers = vec![
            "Name".to_string(),
            "Value".to_string(),
            "Compatibility".to_string(),
        ];
        let mut formatter = TableFormatter::new(headers);

        let row1 = vec![
            "key1".to_string(),
            "value1".to_string(),
            "Compatible".to_string(),
        ];
        let row2 = vec![
            "key2".to_string(),
            "value2".to_string(),
            "Incompatible".to_string(),
        ];
        let row3 = vec![
            "key3".to_string(),
            "value3".to_string(),
            "Unknown".to_string(),
        ];

        formatter.add_row(&row1);
        formatter.add_row(&row2);
        formatter.add_row(&row3);

        let header = formatter.render_header();
        let row1_str = formatter.render_row(&row1, true).green();
        let row2_str = formatter.render_row(&row2, false).red();
        let row3_str = formatter.render_row(&row3, false).yellow();
        let footer = formatter.render_footer();

        assert!(header.contains("Name"));
        assert!(header.contains("Value"));
        assert!(header.contains("Compatibility"));
        assert!(row1_str.contains("key1"));
        assert!(row2_str.contains("key2"));
        assert!(row3_str.contains("key3"));
        assert!(footer.contains("└"));
    }

    #[test]
    fn test_print_incompatible_licenses_table() {
        // Create test data
        let test_data = get_test_data();

        // Create a new Vec that owns the filtered items, rather than borrowing from a temporary
        let incompatible_licenses: Vec<&LicenseInfo> = test_data
            .iter()
            .filter(|info| info.compatibility == LicenseCompatibility::Incompatible)
            .collect();

        assert!(!incompatible_licenses.is_empty());
        print_incompatible_licenses_table(&incompatible_licenses, "MIT");
        // If no panic, test passes
    }

    #[test]
    fn test_print_summary_footer_with_compatibility() {
        // This is primarily a visual test
        let license_info = get_test_data();
        print_summary_footer(&license_info, Some("MIT"));
        // If no panic, test passes
    }

    #[test]
    fn test_print_summary_footer_without_compatibility() {
        // This is primarily a visual test
        let license_info = get_test_data_with_unknown_compatibility();
        print_summary_footer(&license_info, None);
        // If no panic, test passes
    }

    #[test]
    fn test_report_config_default_values() {
        let config = ReportConfig::new(
            false, // json
            false, // yaml
            false, // verbose
            false, // strict
            false, // incompatible
            None,  // ci_format
            None,  // output_file
            None,  // project_license
            false, // gist
            None,  // osi
        );

        assert!(!config.json);
        assert!(!config.yaml);
        assert!(!config.verbose);
        assert!(!config.restrictive);
        assert!(config.ci_format.is_none());
        assert!(config.output_file.is_none());
        assert!(config.project_license.is_none());
    }

    #[test]
    fn test_generate_report_all_permissive() {
        let data = vec![
            LicenseInfo {
                name: "package1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("BSD-3-Clause".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let config = ReportConfig::new(
            false,
            false,
            false,
            false,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let (has_restrictive, has_incompatible) = generate_report(data, config);

        assert!(!has_restrictive);
        assert!(!has_incompatible);
    }

    #[test]
    fn test_generate_report_mixed_licenses() {
        let data = vec![
            LicenseInfo {
                name: "good_package".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "bad_package".to_string(),
                version: "2.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let config = ReportConfig::new(
            false,
            false,
            false,
            false,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let (has_restrictive, has_incompatible) = generate_report(data, config);

        assert!(has_restrictive);
        assert!(has_incompatible);
    }

    #[test]
    fn test_generate_report_strict_mode_filters() {
        let data = vec![
            LicenseInfo {
                name: "permissive_package".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "restrictive_package".to_string(),
                version: "2.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let config = ReportConfig::new(
            false,
            false,
            false,
            true,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let (has_restrictive, has_incompatible) = generate_report(data, config);

        assert!(has_restrictive);
        assert!(has_incompatible);
    }

    #[test]
    fn test_generate_report_json_output() {
        let data = vec![LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let config = ReportConfig::new(
            true, false, false, false, false, None, None, None, false, None,
        );
        let (has_restrictive, has_incompatible) = generate_report(data, config);

        assert!(!has_restrictive);
        assert!(!has_incompatible);
    }

    #[test]
    fn test_generate_report_yaml_output() {
        let data = vec![LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let config = ReportConfig::new(
            false, true, false, false, false, None, None, None, false, None,
        );
        let (has_restrictive, has_incompatible) = generate_report(data, config);

        assert!(!has_restrictive);
        assert!(!has_incompatible);
    }

    #[test]
    fn test_generate_report_verbose_output() {
        let data = vec![LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let config = ReportConfig::new(
            false,
            false,
            true,
            false,
            false,
            None,
            None,
            Some("MIT".to_string()),
            false,
            None,
        );
        let (has_restrictive, has_incompatible) = generate_report(data, config);

        assert!(!has_restrictive);
        assert!(!has_incompatible);
    }

    #[test]
    fn test_github_output_format_stdout() {
        let data = vec![LicenseInfo {
            name: "restrictive_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("GPL-3.0".to_string()),
            is_restrictive: true,
            compatibility: LicenseCompatibility::Incompatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let config = ReportConfig::new(
            false,
            false,
            false,
            false,
            false,
            Some(CiFormat::Github),
            None,
            Some("MIT".to_string()),
            false,
            None,
        );

        let (has_restrictive, has_incompatible) = generate_report(data, config);
        assert!(has_restrictive);
        assert!(has_incompatible);
    }

    #[test]
    fn test_output_github_format_file_write_error() {
        let data = vec![LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        output_github_format(
            &data,
            Some("/invalid/path/that/does/not/exist/output.txt"),
            Some("MIT"),
        );
    }

    #[test]
    fn test_output_jenkins_format_file_write_error() {
        let data = vec![LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        output_jenkins_format(
            &data,
            Some("/invalid/path/that/does/not/exist/output.xml"),
            Some("MIT"),
        );
    }

    #[test]
    fn test_print_restrictive_licenses_table() {
        let data = [
            LicenseInfo {
                name: "restrictive1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "restrictive2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("AGPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let restrictive_refs: Vec<&LicenseInfo> = data.iter().collect();
        print_restrictive_licenses_table(&restrictive_refs);
    }

    #[test]
    fn test_table_formatter_column_width_calculation() {
        let headers = vec!["A".to_string(), "BB".to_string(), "CCC".to_string()];
        let mut formatter = TableFormatter::new(headers);

        assert_eq!(formatter.column_widths[0], 1); // "A"
        assert_eq!(formatter.column_widths[1], 2); // "BB"
        assert_eq!(formatter.column_widths[2], 3); // "CCC"

        let row = vec!["AAAA".to_string(), "B".to_string(), "CC".to_string()];
        formatter.add_row(&row);

        assert_eq!(formatter.column_widths[0], 4); // "AAAA"
        assert_eq!(formatter.column_widths[1], 2); // "BB" (header is longer)
        assert_eq!(formatter.column_widths[2], 3); // "CCC" (header is longer)
    }

    #[test]
    fn test_report_config_debug() {
        let config = ReportConfig::new(
            true,
            false,
            true,
            false,
            false,
            Some(CiFormat::Github),
            Some("test.txt".to_string()),
            Some("MIT".to_string()),
            false,
            None,
        );

        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("ReportConfig"));
        assert!(debug_str.contains("json: true"));
        assert!(debug_str.contains("yaml: false"));
        assert!(debug_str.contains("Github"));
    }
}
