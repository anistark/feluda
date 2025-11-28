use clap::{ArgGroup, Parser, Subcommand, ValueEnum};
use colored::*;
use std::env;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// Import from the debug module instead of defining here
use crate::debug::{is_debug_mode, log, LogLevel};

/// CI output format options
#[derive(ValueEnum, Clone, Debug)]
pub enum CiFormat {
    /// GitHub Actions compatible format
    Github,
    /// Jenkins compatible format (JUnit XML)
    Jenkins,
}

/// SBOM format options
#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum SbomFormat {
    /// SPDX format
    Spdx,
    /// CycloneDX format
    Cyclonedx,
    /// Generate all supported formats
    All,
}

/// OSI filter options
#[derive(ValueEnum, Clone, Debug)]
pub enum OsiFilter {
    /// Show only OSI approved licenses
    Approved,
    /// Show only non-OSI approved licenses
    NotApproved,
    /// Show licenses with unknown OSI status
    Unknown,
}

/// SBOM Subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum SbomCommand {
    /// Generate SPDX format SBOM
    Spdx {
        /// Path to the local project directory
        #[arg(short, long, default_value = "./")]
        path: String,

        /// Path to write the SBOM file
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Generate CycloneDX format SBOM
    Cyclonedx {
        /// Path to the local project directory
        #[arg(short, long, default_value = "./")]
        path: String,

        /// Path to write the SBOM file
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Validate SBOM file (JSON format)
    Validate {
        /// Path to the SBOM file to validate
        #[arg(value_name = "FILE")]
        sbom_file: String,

        /// Path to write the validation report
        #[arg(short, long)]
        output: Option<String>,

        /// Output validation report in JSON format
        #[arg(long)]
        json: bool,
    },
}

/// CLI Commands
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Generate license-related files
    Generate {
        /// Path to the local project directory
        #[arg(short, long, default_value = "./")]
        path: String,

        /// Specify the language to scan
        #[arg(long, short)]
        language: Option<String>,

        /// Specify the project license explicitly
        #[arg(long)]
        project_license: Option<String>,
    },
    /// Generate Software Bill of Materials (SBOM)
    Sbom {
        /// Path to the local project directory
        #[arg(short, long, default_value = "./")]
        path: String,

        /// Path to write the SBOM files
        #[arg(short, long)]
        output: Option<String>,

        /// SBOM format subcommand
        #[command(subcommand)]
        format: Option<SbomCommand>,
    },
    /// Manage cache
    Cache {
        /// Clear the GitHub licenses cache
        #[arg(long)]
        clear: bool,
    },
}

#[derive(Parser, Debug, Clone)]
#[command(author, version)]
#[command(about = env!("CARGO_PKG_DESCRIPTION"))]
#[command(
    long_about = "Feluda is a CLI tool that analyzes the dependencies of a project, identifies their licenses, and flags any that may restrict personal or commercial usage."
)]
#[command(group(ArgGroup::new("output").args(["json"])))]
#[command(group(ArgGroup::new("source").args(["path", "repo"]).multiple(false)))] // Mutually exclusive path and repo
#[command(before_help = format_before_help())]
pub struct Cli {
    /// Enable debug mode
    #[arg(long, short, global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to the local project directory
    #[arg(short, long, default_value = "./")]
    pub path: String,

    /// URL of the Git repository to analyze (HTTPS or SSH)
    #[arg(long)]
    pub repo: Option<String>,

    // For HTTPS authentication
    #[arg(long, requires = "repo")]
    pub token: Option<String>,

    // For custom SSH key path
    #[arg(long, requires = "repo")]
    pub ssh_key: Option<String>,

    // For custom SSH key passphrase
    #[arg(long)]
    pub ssh_passphrase: Option<String>,

    /// Output in JSON format
    #[arg(long, short, group = "output")]
    /// This will override the default output format
    /// and will not show the TUI table.
    /// This is useful for CI/CD pipelines.
    pub json: bool,

    /// Output in YAML format
    #[arg(long, short, group = "output")]
    /// This will override the default output format
    /// and will not show the TUI table.
    /// This is useful for CI/CD pipelines.
    pub yaml: bool,

    /// Enable verbose output
    #[arg(long)]
    pub verbose: bool,

    /// Show only restrictive dependencies
    #[arg(long, short)]
    pub restrictive: bool,

    /// Enable TUI table
    #[arg(long, short)]
    pub gui: bool,

    /// Specify the language to scan
    #[arg(long, short)]
    pub language: Option<String>,

    /// Output format for CI systems (github, jenkins)
    #[arg(long, value_enum)]
    pub ci_format: Option<CiFormat>,

    /// Path to write the CI report file
    #[arg(long)]
    pub output_file: Option<String>,

    /// Fail with non-zero exit code when restrictive licenses are found
    #[arg(long)]
    pub fail_on_restrictive: bool,

    /// Show only incompatible dependencies
    #[arg(long)]
    pub incompatible: bool,

    /// Fail with non-zero exit code when incompatible licenses are found
    #[arg(long)]
    pub fail_on_incompatible: bool,

    /// Specify the project license (overrides auto-detection)
    #[arg(long)]
    pub project_license: Option<String>,

    // Show a concise gist summary
    #[arg(long, group = "output")]
    pub gist: bool,

    /// Filter by OSI license approval status
    #[arg(long, value_enum)]
    pub osi: Option<OsiFilter>,

    /// Enable strict mode for license parser
    #[arg(long)]
    pub strict: bool,

    /// Skip local license detection, force network lookup only
    #[arg(long)]
    pub no_local: bool,
}

impl Cli {
    /// Get the command arguments
    pub fn get_command_args(&self) -> Commands {
        match &self.command {
            Some(cmd) => cmd.clone(),
            None => {
                // No subcommand provided - default to license analysis
                Commands::Generate {
                    path: "".to_string(),
                    language: None,
                    project_license: None,
                }
            }
        }
    }

    /// Check if this is the default behavior
    pub fn is_default_command(&self) -> bool {
        self.command.is_none()
    }
}

fn format_before_help() -> String {
    format!(
        "{}\n{}\n{}",
        "┌───────────────────────────────────────────┐".bright_cyan(),
        "│           FELUDA LICENSE CHECKER          │"
            .bright_cyan()
            .bold(),
        "└───────────────────────────────────────────┘".bright_cyan()
    )
}

// Function to print a customized version info
pub fn print_version_info() {
    // Get version from Cargo.toml using env!
    let version = env!("CARGO_PKG_VERSION");
    let title = format!("Feluda v{version}");
    let width = title.len() + 4;
    let border = "─".repeat(width);

    println!("{}", format!("┌{border}┐").bright_red());
    println!(
        "{}",
        format!("│ {}   │", title.bright_white().bold()).bright_red()
    );
    println!("{}", format!("└{border}┘").bright_red());
    println!(
        "{}",
        "\nA dependency license checker written in Rust.".bright_yellow()
    );
    println!(
        "{}",
        "Checks for permissive and restrictive licenses.".bright_yellow()
    );
    println!(
        "{}",
        "\nFound Feluda useful? ✨ Star the repository:"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "https://github.com/anistark/feluda".blue().underline()
    );
}

/// A loading indicator that displays a spinner and progress updates
/// without deleting the previous line
pub struct LoadingIndicator {
    message: String,
    running: Arc<AtomicBool>,
    spinner_frames: Vec<&'static str>,
    handle: Option<thread::JoinHandle<()>>,
    progress: Arc<Mutex<Option<String>>>,
}

impl LoadingIndicator {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            running: Arc::new(AtomicBool::new(true)),
            spinner_frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            handle: None,
            progress: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&mut self) {
        if is_debug_mode() {
            // In debug mode, just log the message without spinner
            log(LogLevel::Info, &format!("Operation: {}", self.message));
            return;
        }

        let message = self.message.clone();
        let running = self.running.clone();
        let spinner_frames = self.spinner_frames.clone();
        let progress = self.progress.clone();

        // Clear the current line and move to beginning
        print!("\x1B[2K\r");

        // Print initial message with spinner
        print!("{} {} ", spinner_frames[0].cyan(), message);
        io::stdout().flush().unwrap();

        let handle = thread::spawn(move || {
            let mut frame_idx = 0;
            while running.load(Ordering::Relaxed) {
                frame_idx = (frame_idx + 1) % spinner_frames.len();

                // Clear the current line and move to beginning
                print!("\x1B[2K\r");

                // Print spinner and message
                let spinner_char = spinner_frames[frame_idx];
                print!("{} {} ", spinner_char.cyan(), message);

                // Print progress info if available
                if let Some(ref progress_text) = *progress.lock().unwrap() {
                    print!("({progress_text})");
                }

                io::stdout().flush().unwrap();
                thread::sleep(Duration::from_millis(80));
            }

            // Clear line and print completion message
            print!("\x1B[2K\r");
            print!("{} {} ", "✓".green().bold(), message);
            if let Some(ref progress_text) = *progress.lock().unwrap() {
                print!("({progress_text})");
            }
            println!(" ✅");
            io::stdout().flush().unwrap();
        });

        self.handle = Some(handle);
    }

    pub fn update_progress(&self, progress_text: &str) {
        if let Ok(mut guard) = self.progress.lock() {
            *guard = Some(progress_text.to_string());
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // Wait for spinner thread to finish its final update
            let _ = handle.join();
        }
    }
}

/// Execute a function with a loading indicator
///
/// This function provides a loading indicator with spinner while the provided
/// function is running. The function is passed a reference to the loading
/// indicator, which can be used to update the progress display.
///
/// # Examples
///
/// ```
/// let result = with_spinner("Processing data", |indicator| {
///     // Initial work
///     let data = prepare_data();
///
///     // Update progress
///     indicator.update_progress(&format!("processed {} items", data.len()));
///
///     // Continue processing
///     process_data(data)
/// });
/// ```
pub fn with_spinner<F, T>(message: &str, f: F) -> T
where
    F: FnOnce(&LoadingIndicator) -> T,
{
    if is_debug_mode() {
        log(LogLevel::Info, &format!("Operation: {message}"));
        let start = std::time::Instant::now();
        let indicator = LoadingIndicator::new(message);
        let result = f(&indicator);
        let duration = start.elapsed();
        log(LogLevel::Info, &format!("Completed in {duration:?}"));
        result
    } else {
        let mut indicator = LoadingIndicator::new(message);
        indicator.start();
        let result = f(&indicator);
        indicator.stop();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loading_indicator() {
        // This is a simple test to ensure the LoadingIndicator can be created and used
        let indicator = LoadingIndicator::new("Test operation"); // Removed 'mut' as it's unused
        indicator.update_progress("step 1");
        indicator.update_progress("step 2");
        // In a real test, we would start the indicator but that would create output
        // during tests, so we'll skip that part
        assert!(indicator.handle.is_none());
    }

    #[test]
    fn test_with_spinner() {
        // Test using with_spinner for a simple operation
        let result = with_spinner("Test operation", |indicator| {
            indicator.update_progress("working");
            // Return value directly instead of using an intermediate variable
            42
        });

        assert_eq!(result, 42);
    }

    #[test]
    fn test_cli_default_values() {
        let cli = Cli {
            debug: false,
            command: None,
            path: "./".to_string(),
            repo: None,
            token: None,
            ssh_key: None,
            ssh_passphrase: None,
            json: false,
            yaml: false,
            verbose: false,
            restrictive: false,
            gui: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
            gist: false,
            osi: None,
            strict: false,
            no_local: false,
        };

        assert_eq!(cli.path, "./");
        assert!(!cli.debug);
        assert!(!cli.json);
        assert!(!cli.restrictive);
        assert!(!cli.strict);
        assert!(!cli.no_local);
        assert!(cli.is_default_command());
    }

    #[test]
    fn test_get_command_args_with_command() {
        let cli = Cli {
            debug: false,
            command: Some(Commands::Generate {
                path: "/test/path".to_string(),
                language: Some("rust".to_string()),
                project_license: Some("MIT".to_string()),
            }),
            path: "./".to_string(),
            repo: None,
            token: None,
            ssh_key: None,
            ssh_passphrase: None,
            json: false,
            yaml: false,
            verbose: false,
            restrictive: false,
            gui: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
            gist: false,
            osi: None,
            strict: false,
            no_local: false,
        };

        let cmd = cli.get_command_args();
        match cmd {
            Commands::Generate {
                path,
                language,
                project_license,
            } => {
                assert_eq!(path, "/test/path");
                assert_eq!(language, Some("rust".to_string()));
                assert_eq!(project_license, Some("MIT".to_string()));
            }
            Commands::Sbom { .. } => {
                panic!("Expected Generate command");
            }
            Commands::Cache { .. } => {
                panic!("Expected Generate command");
            }
        }
        assert!(!cli.is_default_command());
    }

    #[test]
    fn test_get_command_args_default() {
        let cli = Cli {
            debug: false,
            command: None,
            path: "./test".to_string(),
            repo: None,
            token: None,
            ssh_key: None,
            ssh_passphrase: None,
            json: false,
            yaml: false,
            verbose: false,
            restrictive: false,
            gui: false,
            language: None,
            ci_format: None,
            output_file: None,
            fail_on_restrictive: false,
            incompatible: false,
            fail_on_incompatible: false,
            project_license: None,
            gist: false,
            osi: None,
            strict: false,
            no_local: false,
        };

        let cmd = cli.get_command_args();
        match cmd {
            Commands::Generate {
                path,
                language,
                project_license,
            } => {
                assert_eq!(path, "");
                assert_eq!(language, None);
                assert_eq!(project_license, None);
            }
            Commands::Sbom { .. } => {
                panic!("Expected Generate command");
            }
            Commands::Cache { .. } => {
                panic!("Expected Generate command");
            }
        }
    }

    #[test]
    fn test_loading_indicator_new() {
        let indicator = LoadingIndicator::new("Test message");
        assert_eq!(indicator.message, "Test message");
        assert!(indicator.running.load(Ordering::Relaxed));
        assert!(indicator.handle.is_none());
        assert_eq!(indicator.spinner_frames.len(), 10);
    }

    #[test]
    fn test_loading_indicator_update_progress() {
        let indicator = LoadingIndicator::new("Test");
        indicator.update_progress("step 1");

        let progress = indicator.progress.lock().unwrap();
        assert_eq!(*progress, Some("step 1".to_string()));

        drop(progress);
        indicator.update_progress("step 2");

        let progress = indicator.progress.lock().unwrap();
        assert_eq!(*progress, Some("step 2".to_string()));
    }

    #[test]
    fn test_with_spinner_execution() {
        let result = with_spinner("Test operation", |indicator| {
            indicator.update_progress("working");
            42
        });
        assert_eq!(result, 42);
    }

    #[test]
    fn test_with_spinner_with_error() {
        let result = std::panic::catch_unwind(|| {
            with_spinner("Test operation", |_indicator| {
                panic!("Test panic");
            })
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_format_before_help() {
        let help_text = format_before_help();
        assert!(help_text.contains("FELUDA LICENSE CHECKER"));
        assert!(help_text.contains("┌"));
        assert!(help_text.contains("└"));
        assert!(help_text.contains("│"));
    }

    #[test]
    fn test_print_version_info() {
        print_version_info();
    }

    #[test]
    fn test_ci_format_enum() {
        let github = CiFormat::Github;
        let jenkins = CiFormat::Jenkins;

        assert_ne!(format!("{github:?}"), format!("{:?}", jenkins));

        let github_clone = github.clone();
        assert_eq!(format!("{github:?}"), format!("{:?}", github_clone));
    }

    #[test]
    fn test_commands_enum_clone() {
        let generate_cmd = Commands::Generate {
            path: "./".to_string(),
            language: None,
            project_license: None,
        };

        let cloned_cmd = generate_cmd.clone();

        match (generate_cmd, cloned_cmd) {
            (
                Commands::Generate {
                    path: p1,
                    language: l1,
                    project_license: pl1,
                },
                Commands::Generate {
                    path: p2,
                    language: l2,
                    project_license: pl2,
                },
            ) => {
                assert_eq!(p1, p2);
                assert_eq!(l1, l2);
                assert_eq!(pl1, pl2);
            }
            _ => {
                panic!("Expected both commands to be Generate");
            }
        }
    }

    #[test]
    fn test_loading_indicator_multiple_progress_updates() {
        let indicator = LoadingIndicator::new("Multi-step test");

        for i in 1..=5 {
            indicator.update_progress(&format!("step {i}"));
            let progress = indicator.progress.lock().unwrap();
            assert_eq!(*progress, Some(format!("step {i}")));
            drop(progress);
        }
    }

    #[test]
    fn test_sbom_command_default_all() {
        let sbom_cmd = Commands::Sbom {
            path: "./".to_string(),
            format: None,
            output: None,
        };

        match sbom_cmd {
            Commands::Sbom {
                path,
                format,
                output,
            } => {
                assert_eq!(path, "./");
                assert!(format.is_none());
                assert!(output.is_none());
            }
            _ => panic!("Expected Sbom command"),
        }
    }

    #[test]
    fn test_sbom_command_spdx() {
        let sbom_cmd = Commands::Sbom {
            path: "/project".to_string(),
            format: Some(SbomCommand::Spdx {
                path: "/project".to_string(),
                output: Some("sbom.json".to_string()),
            }),
            output: None,
        };

        match sbom_cmd {
            Commands::Sbom {
                path,
                format,
                output,
            } => {
                assert_eq!(path, "/project");
                assert!(format.is_some());
                assert!(output.is_none());
                match format.unwrap() {
                    SbomCommand::Spdx { path: p, output: o } => {
                        assert_eq!(p, "/project");
                        assert_eq!(o, Some("sbom.json".to_string()));
                    }
                    _ => panic!("Expected Spdx subcommand"),
                }
            }
            _ => panic!("Expected Sbom command"),
        }
    }

    #[test]
    fn test_sbom_command_cyclonedx() {
        let sbom_cmd = Commands::Sbom {
            path: "/project".to_string(),
            format: Some(SbomCommand::Cyclonedx {
                path: "/project".to_string(),
                output: Some("sbom.xml".to_string()),
            }),
            output: None,
        };

        match sbom_cmd {
            Commands::Sbom {
                path,
                format,
                output,
            } => {
                assert_eq!(path, "/project");
                assert!(format.is_some());
                assert!(output.is_none());
                match format.unwrap() {
                    SbomCommand::Cyclonedx { path: p, output: o } => {
                        assert_eq!(p, "/project");
                        assert_eq!(o, Some("sbom.xml".to_string()));
                    }
                    _ => panic!("Expected Cyclonedx subcommand"),
                }
            }
            _ => panic!("Expected Sbom command"),
        }
    }
}
