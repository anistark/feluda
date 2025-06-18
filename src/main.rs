mod cli;
mod config;
mod debug;
mod generate;
mod licenses;
mod parser;
mod reporter;
mod table;
mod utils;

use clap::Parser;
use cli::{print_version_info, Cli, Commands};
use debug::{log, log_debug, set_debug_mode, FeludaError, FeludaResult, LogLevel};
use generate::handle_generate_command;
use licenses::{detect_project_license, is_license_compatible, LicenseCompatibility};
use parser::parse_root;
use reporter::{generate_report, ReportConfig};
use std::env;
use std::path::Path;
use std::process;
use table::App;
use tempfile::TempDir;
use utils::clone_repository;

/// Configuration for the check command
#[derive(Debug)]
struct CheckConfig {
    path: String,
    json: bool,
    yaml: bool,
    verbose: bool,
    strict: bool,
    gui: bool,
    language: Option<String>,
    ci_format: Option<cli::CiFormat>,
    output_file: Option<String>,
    fail_on_restrictive: bool,
    incompatible: bool,
    fail_on_incompatible: bool,
    project_license: Option<String>,
}

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
        log(
            LogLevel::Info,
            &format!("Starting Feluda with args: {:?}", args),
        );
    }

    // Handle repository cloning if --repo is provided
    let (analysis_path, _temp_dir) = match &args.repo.clone() {
        Some(repo_url) => {
            log(
                LogLevel::Info,
                &format!("Attempting to clone repository: {}", repo_url),
            );
            let temp_dir = TempDir::new().map_err(|e| {
                FeludaError::Unknown(format!("Failed to create temporary directory: {}", e))
            })?;
            let repo_path = temp_dir.path();

            // Clone the repository
            if let Err(e) = clone_repository(&args, repo_path) {
                log(
                    LogLevel::Error,
                    &format!("Repository cloning failed: {}", e),
                );
                return Err(e);
            }
            log(
                LogLevel::Info,
                &format!("Repository cloned to: {}", repo_path.display()),
            );
            (repo_path.to_path_buf(), Some(temp_dir))
        }
        None => {
            let path = Path::new(&args.path).to_path_buf();
            log(
                LogLevel::Info,
                &format!("Using local path for analysis: {}", path.display()),
            );
            (path, None)
        }
    };

    log(
        LogLevel::Info,
        &format!("Analysing project at: {}", analysis_path.display()),
    );

    // Handle the command based on whether a subcommand was provided
    if args.is_default_command() {
        // Default behavior: license analysis
        let config = CheckConfig {
            path: analysis_path.to_string_lossy().to_string(),
            json: args.json,
            yaml: args.yaml,
            verbose: args.verbose,
            strict: args.strict,
            gui: args.gui,
            language: args.language,
            ci_format: args.ci_format,
            output_file: args.output_file,
            fail_on_restrictive: args.fail_on_restrictive,
            incompatible: args.incompatible,
            fail_on_incompatible: args.fail_on_incompatible,
            project_license: args.project_license,
        };
        handle_check_command(config)
    } else {
        // Handle subcommands
        let command = args.get_command_args();
        match command {
            Commands::Generate {
                path,
                language,
                project_license,
            } => {
                handle_generate_command(path, language, project_license);
                Ok(())
            }
        }
    }
}

fn handle_check_command(config: CheckConfig) -> FeludaResult<()> {
    log(
        LogLevel::Info,
        &format!("Executing check command with path: {}", config.path),
    );

    // Parse project dependencies
    log(
        LogLevel::Info,
        &format!("Parsing dependencies in path: {}", config.path),
    );

    let mut project_license = config.project_license;

    // If no project license is provided via CLI, try to detect it
    if project_license.is_none() {
        log(
            LogLevel::Info,
            "No project license specified, attempting to detect",
        );
        match detect_project_license(&config.path) {
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
    let mut analyzed_data = parse_root(&config.path, config.language.as_deref())
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

    // Either run the GUI or generate a report
    if config.gui {
        let original_count = analyzed_data.len();

        // Filter for restrictive and incompatible
        if config.strict || config.incompatible {
            if project_license.is_some() {
                log(
                LogLevel::Info,
                "Strict and incompatible mode enabled, filtering for restrictive and incompatible licenses",
            );
                analyzed_data.retain(|info| {
                    (config.strict && *info.is_restrictive())
                        || (config.incompatible
                            && info.compatibility == LicenseCompatibility::Incompatible)
                });

                log(
                    LogLevel::Info,
                    &format!(
                        "Filtered for restrictive and incompatible licenses: {} of {} dependencies",
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
        } else if config.strict {
            // Filter for restrictive
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
        } else if config.incompatible {
            // Filter for incompatible if requested
            if project_license.is_some() {
                log(
                    LogLevel::Info,
                    "Incompatible mode enabled, filtering for incompatible licenses",
                );
                analyzed_data
                    .retain(|info| info.compatibility == LicenseCompatibility::Incompatible);

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

        // Create ReportConfig from CLI arguments
        let report_config = ReportConfig::new(
            config.json,
            config.yaml,
            config.verbose,
            config.strict,
            config.incompatible,
            config.ci_format,
            config.output_file,
            project_license,
        );

        // Generate a report based on the analyzed data
        let (has_restrictive, has_incompatible) = generate_report(analyzed_data, report_config);

        log(
            LogLevel::Info,
            &format!(
                "Report generated, has_restrictive: {}, has_incompatible: {}",
                has_restrictive, has_incompatible
            ),
        );

        if (config.fail_on_restrictive && has_restrictive)
            || (config.fail_on_incompatible && has_incompatible)
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
