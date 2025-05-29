mod cli;
mod config;
mod debug;
mod licenses;
mod parser;
mod reporter;
mod table;

use clap::Parser;
use cli::{print_version_info, Cli};
use debug::{log, log_debug, set_debug_mode, FeludaError, FeludaResult, LogLevel};
use licenses::{detect_project_license, is_license_compatible, LicenseCompatibility};
use parser::parse_root;
use reporter::generate_report;
use std::env;
use std::process;
use table::App;

fn main() {
    // Check if --version or -V is passed alone
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        print_version_info();
        return;
    }

    match run() {
        Ok(_) => {}
        Err(e) => {
            e.log();
            process::exit(1);
        }
    }
}

fn run() -> FeludaResult<()> {
    let args = Cli::parse();

    // Debug mode
    if args.debug {
        set_debug_mode(true);
    }

    log(
        LogLevel::Info,
        &format!("Starting Feluda with args: {:?}", args),
    );

    // Parse project dependencies
    log(
        LogLevel::Info,
        &format!("Parsing dependencies in path: {}", args.path),
    );

    // Get project license from CLI args if provided
    let mut project_license = args.project_license.clone();

    // If no project license is provided via CLI, try to detect it
    if project_license.is_none() {
        log(
            LogLevel::Info,
            "No project license specified, attempting to detect",
        );
        match detect_project_license(&args.path) {
            Ok(Some(detected)) => {
                log(
                    LogLevel::Info,
                    &format!("Detected project license: {}", detected),
                );
                project_license = Some(detected);
            }
            Ok(None) => {
                log(LogLevel::Warn, "Could not detect project license");
            }
            Err(e) => {
                log(
                    LogLevel::Error,
                    &format!("Error detecting project license: {}", e),
                );
            }
        }
    } else {
        log(
            LogLevel::Info,
            &format!(
                "Using provided project license: {}",
                project_license.as_ref().unwrap()
            ),
        );
    }

    // Parse and analyze dependencies
    let mut analyzed_data = parse_root(&args.path, args.language.as_deref())
        .map_err(|e| FeludaError::Parser(format!("Failed to parse dependencies: {}", e)))?;

    log_debug("Analyzed dependencies", &analyzed_data);

    // Update each dependency with compatibility information if project license is known
    if let Some(ref proj_license) = project_license {
        log(
            LogLevel::Info,
            &format!(
                "Checking license compatibility against project license: {}",
                proj_license
            ),
        );

        for info in &mut analyzed_data {
            if let Some(ref dep_license) = info.license {
                info.compatibility = is_license_compatible(dep_license, proj_license);

                log(
                    LogLevel::Info,
                    &format!(
                        "License compatibility for {} ({}): {:?}",
                        info.name, dep_license, info.compatibility
                    ),
                );
            } else {
                info.compatibility = LicenseCompatibility::Unknown;

                log(
                    LogLevel::Info,
                    &format!(
                        "License compatibility for {} unknown (no license info)",
                        info.name
                    ),
                );
            }
        }
    } else {
        // If no project license is known, mark all as unknown compatibility
        log(
            LogLevel::Warn,
            "No project license specified or detected, marking all dependencies as unknown compatibility",
        );

        for info in &mut analyzed_data {
            info.compatibility = LicenseCompatibility::Unknown;
        }
    }

    // Filter for restrictive if in strict mode
    let original_count = analyzed_data.len();

    if args.strict {
        log(
            LogLevel::Info,
            "Strict mode enabled, filtering for restrictive licenses",
        );
        analyzed_data.retain(|info| *info.is_restrictive());

        log(
            LogLevel::Info,
            &format!(
                "Filtered for restrictive licenses: {} of {} dependencies",
                analyzed_data.len(),
                original_count
            ),
        );
    }

    // Filter for incompatible if requested
    if args.incompatible {
        if project_license.is_some() {
            log(
                LogLevel::Info,
                "Incompatible mode enabled, filtering for incompatible licenses",
            );
            analyzed_data.retain(|info| info.compatibility == LicenseCompatibility::Incompatible);

            log(
                LogLevel::Info,
                &format!(
                    "Filtered for incompatible licenses: {} of {} dependencies",
                    analyzed_data.len(),
                    original_count
                ),
            );
        } else {
            log(
                LogLevel::Warn,
                "Incompatible mode enabled but no project license specified, cannot filter for incompatible licenses",
            );
        }
    }

    // Either run the GUI or generate a report
    if args.gui {
        log(LogLevel::Info, "Starting TUI mode");

        // Initialize the terminal
        color_eyre::install()
            .map_err(|e| FeludaError::Unknown(format!("Failed to initialize color_eyre: {}", e)))?;

        let terminal = ratatui::init();
        log(LogLevel::Info, "Terminal initialized for TUI");

        // TUI app with project license info
        let app_result = App::new(analyzed_data, project_license).run(terminal);
        ratatui::restore();

        // Handle any errors from the TUI
        app_result.map_err(|e| FeludaError::Unknown(format!("TUI error: {}", e)))?;

        log(LogLevel::Info, "TUI session completed successfully");
    } else {
        log(LogLevel::Info, "Generating dependency report");

        // Generate a report based on the analyzed data
        let (has_restrictive, has_incompatible) = generate_report(
            analyzed_data,
            args.json,
            args.yaml,
            args.verbose,
            args.strict,
            args.ci_format,
            args.output_file.clone(),
            project_license,
        );

        log(
            LogLevel::Info,
            &format!(
                "Report generated, has_restrictive: {}, has_incompatible: {}",
                has_restrictive, has_incompatible
            ),
        );

        // Exit with non-zero code if requested and issues found
        if (args.fail_on_restrictive && has_restrictive)
            || (args.fail_on_incompatible && has_incompatible)
        {
            log(
                LogLevel::Warn,
                "Exiting with non-zero status due to license issues",
            );
            process::exit(1);
        }
    }

    log(LogLevel::Info, "Feluda completed successfully");
    Ok(())
}
