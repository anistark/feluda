use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{ArgGroup, Parser, Subcommand, ValueEnum};
use colored::*;
use std::env;
use std::io::{self, IsTerminal, Write};
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
    /// SARIF 2.1.0 format (GitHub Advanced Security, VS Code Problems panel)
    Sarif,
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
    /// Initialise Feluda in the current project (generates .feluda.toml and .pre-commit-config.yaml)
    Init {
        /// Path to the local project directory
        #[arg(short, long, default_value = "./")]
        path: String,

        /// Overwrite existing config files without prompting
        #[arg(long)]
        force: bool,

        /// Skip creating or updating .pre-commit-config.yaml
        #[arg(long)]
        no_pre_commit: bool,
    },
    /// Continuously re-scan when dependency files change (filesystem watch)
    Watch {
        /// Path to the local project directory
        #[arg(short, long, default_value = "./")]
        path: String,

        /// Milliseconds to wait after a change before re-scanning (debounce window)
        #[arg(long, default_value_t = 500)]
        debounce: u64,
    },
}

/// Styling for clap's generated help, matching Feluda's cyan branding
const HELP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default())
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Green.on_default())
    .invalid(AnsiColor::Yellow.on_default());

const HEADING_SOURCE: &str = "Project Source";
const HEADING_OUTPUT: &str = "Output";
const HEADING_FILTERS: &str = "Filters";
const HEADING_CI: &str = "CI Integration";
const HEADING_DETECTION: &str = "License Detection";

#[derive(Parser, Debug, Clone)]
#[command(author, version)]
#[command(about = env!("CARGO_PKG_DESCRIPTION"))]
#[command(
    long_about = "Feluda is a CLI tool that analyzes the dependencies of a project, identifies their licenses, and flags any that may restrict personal or commercial usage."
)]
#[command(group(ArgGroup::new("output").args(["json"])))]
#[command(group(ArgGroup::new("source").args(["path", "repo"]).multiple(false)))] // Mutually exclusive path and repo
#[command(before_help = format_before_help())]
#[command(after_help = format_after_help())]
#[command(styles = HELP_STYLES)]
pub struct Cli {
    /// Enable debug mode
    #[arg(long, short, global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to the local project directory
    #[arg(short, long, default_value = "./", help_heading = HEADING_SOURCE)]
    pub path: String,

    /// URL of the Git repository to analyze (HTTPS or SSH)
    #[arg(long, help_heading = HEADING_SOURCE)]
    pub repo: Option<String>,

    /// Access token for HTTPS repository authentication
    #[arg(long, requires = "repo", help_heading = HEADING_SOURCE)]
    pub token: Option<String>,

    /// Path to the SSH private key for repository authentication
    #[arg(long, requires = "repo", help_heading = HEADING_SOURCE)]
    pub ssh_key: Option<String>,

    /// Passphrase for the SSH private key
    #[arg(long, help_heading = HEADING_SOURCE)]
    pub ssh_passphrase: Option<String>,

    /// GitHub personal access token for API authentication (increases rate limits)
    #[arg(long, env = "GITHUB_TOKEN", global = true, help_heading = HEADING_SOURCE)]
    pub github_token: Option<String>,

    /// Output in JSON format (skips the TUI table, useful for CI/CD)
    #[arg(long, short, group = "output", help_heading = HEADING_OUTPUT)]
    pub json: bool,

    /// Output in YAML format (skips the TUI table, useful for CI/CD)
    #[arg(long, short, group = "output", help_heading = HEADING_OUTPUT)]
    pub yaml: bool,

    /// Enable verbose output
    #[arg(long, help_heading = HEADING_OUTPUT)]
    pub verbose: bool,

    /// Show only restrictive dependencies
    #[arg(long, short, help_heading = HEADING_FILTERS)]
    pub restrictive: bool,

    /// Enable TUI table
    #[arg(long, short, help_heading = HEADING_OUTPUT)]
    pub gui: bool,

    /// Specify the language to scan
    #[arg(long, short, help_heading = HEADING_FILTERS)]
    pub language: Option<String>,

    /// Output format for CI systems (github, jenkins, sarif)
    #[arg(long, value_enum, help_heading = HEADING_CI)]
    pub ci_format: Option<CiFormat>,

    /// Path to write the CI report file
    #[arg(long, help_heading = HEADING_CI)]
    pub output_file: Option<String>,

    /// Fail with non-zero exit code when restrictive licenses are found
    #[arg(long, help_heading = HEADING_CI)]
    pub fail_on_restrictive: bool,

    /// Show only incompatible dependencies
    #[arg(long, help_heading = HEADING_FILTERS)]
    pub incompatible: bool,

    /// Fail with non-zero exit code when incompatible licenses are found
    #[arg(long, help_heading = HEADING_CI)]
    pub fail_on_incompatible: bool,

    /// Specify the project license (overrides auto-detection)
    #[arg(long, help_heading = HEADING_DETECTION)]
    pub project_license: Option<String>,

    /// Show a concise summary of the scan
    #[arg(long, group = "output", help_heading = HEADING_OUTPUT)]
    pub gist: bool,

    /// Filter by OSI license approval status
    #[arg(long, value_enum, help_heading = HEADING_FILTERS)]
    pub osi: Option<OsiFilter>,

    /// Enable strict mode for license parser
    #[arg(long, help_heading = HEADING_DETECTION)]
    pub strict: bool,

    /// Skip local license detection, force network lookup only
    #[arg(long, help_heading = HEADING_DETECTION)]
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

/// The FELUDA wordmark, rendered from the 5x7 glyph bitmaps of the
/// Pixelspace typeface (https://github.com/anistark/pixelspace).
/// Quadrant blocks pack two pixel rows per terminal row while keeping
/// the space between pixels that gives the typeface its name.
const FELUDA_PIXELS: [&str; 4] = [
    "▌▘▘▘▘ ▌▘▘▘▘ ▌     ▌   ▌ ▌▘▘▘▖ ▖▘▘▘▖",
    "▌▖▖▖  ▌▖▖▖  ▌     ▌   ▌ ▌   ▌ ▌▖▖▖▌",
    "▌     ▌     ▌     ▌   ▌ ▌   ▌ ▌   ▌",
    "▘     ▘▘▘▘▘ ▘▘▘▘▘  ▘▘▘  ▘▘▘▘  ▘   ▘",
];

/// Render the Pixelspace wordmark with an optional column of text on
/// the right, preceded by one blank line. Rows beyond the wordmark
/// height are indented to stay in the right column.
fn render_banner(right_lines: &[String]) -> String {
    // Block characters are multibyte, so measure in chars, matching how
    // format! width pads
    let art_width = FELUDA_PIXELS[0].chars().count();
    let rows = FELUDA_PIXELS.len().max(right_lines.len());
    let mut lines = vec![String::new()];
    lines.extend((0..rows).map(|i| {
        let left = FELUDA_PIXELS.get(i).copied().unwrap_or("");
        let right = right_lines.get(i).map(String::as_str).unwrap_or("");
        if right.is_empty() {
            format!("  {}", left.trim_end().bright_cyan().bold())
        } else {
            format!(
                "  {}   {}",
                format!("{left:<art_width$}").bright_cyan().bold(),
                right
            )
        }
    }));
    lines.join("\n")
}

fn format_before_help() -> String {
    render_banner(&[
        String::new(),
        format!("Feluda v{}", env!("CARGO_PKG_VERSION"))
            .bright_white()
            .bold()
            .to_string(),
        "https://feluda.readthedocs.io"
            .blue()
            .underline()
            .to_string(),
    ])
}

fn format_after_help() -> String {
    let example = |cmd: &str, desc: &str| {
        format!(
            "  {} {}",
            format!("{cmd:<48}").green().bold(),
            desc.dimmed()
        )
    };
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n\n{} {}",
        "Examples:".bright_cyan().bold(),
        example("feluda", "Scan the current directory"),
        example("feluda --path ../my-project", "Scan another local project"),
        example(
            "feluda --repo https://github.com/user/repo",
            "Scan a remote repository"
        ),
        example("feluda --json", "Machine-readable output for pipelines"),
        example(
            "feluda --ci-format github --fail-on-restrictive",
            "Gate a CI run on restrictive licenses",
        ),
        "Learn more:".bright_cyan().bold(),
        env!("CARGO_PKG_REPOSITORY").blue().underline()
    )
}

/// Latest published release, fetched from the GitHub releases API
struct LatestRelease {
    version: String,
    notes: Vec<String>,
    url: String,
}

/// Fetch the latest release from GitHub. Returns None on any failure
/// (offline, rate limited, unexpected payload) so the caller can degrade
/// gracefully.
fn fetch_latest_release() -> Option<LatestRelease> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("feluda-license-checker/1.0")
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;
    let response = client
        .get("https://api.github.com/repos/anistark/feluda/releases/latest")
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let json: serde_json::Value = response.json().ok()?;
    let version = json
        .get("tag_name")?
        .as_str()?
        .trim_start_matches('v')
        .to_string();
    let url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://github.com/anistark/feluda/releases")
        .to_string();
    let notes = json
        .get("body")
        .and_then(|v| v.as_str())
        .map(extract_release_bullets)
        .unwrap_or_default();
    Some(LatestRelease {
        version,
        notes,
        url,
    })
}

/// Pull the first few bullet points out of a markdown release body,
/// dropping the "by @user in <PR url>" attribution suffix.
fn extract_release_bullets(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let text = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))?;
            let text = text.split(" by @").next().unwrap_or(text).trim();
            if text.is_empty() {
                None
            } else {
                Some(truncate_chars(text, 70))
            }
        })
        .take(5)
        .collect()
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

/// How this feluda binary was installed, used to suggest the right
/// upgrade command
#[derive(Debug, PartialEq)]
enum InstallMethod {
    Homebrew,
    Cargo,
    Aur,
    SystemPackage,
    Unknown,
}

impl InstallMethod {
    fn upgrade_line(&self) -> String {
        let command = match self {
            InstallMethod::Homebrew => "brew upgrade feluda",
            InstallMethod::Aur => "paru -S feluda",
            InstallMethod::SystemPackage => {
                return "Download from the releases page".yellow().to_string()
            }
            InstallMethod::Cargo | InstallMethod::Unknown => "cargo install feluda",
        };
        format!("{} {}", "Upgrade with:".yellow(), command.green().bold())
    }
}

fn detect_install_method(
    exe_path: &str,
    cargo_home: Option<&str>,
    has_arch_release: bool,
    has_distro_package_manager: bool,
) -> InstallMethod {
    if exe_path.contains("/Cellar/")
        || exe_path.contains("homebrew")
        || exe_path.contains("linuxbrew")
    {
        return InstallMethod::Homebrew;
    }
    if exe_path.contains(".cargo")
        || cargo_home.is_some_and(|home| !home.is_empty() && exe_path.starts_with(home))
    {
        return InstallMethod::Cargo;
    }
    if exe_path.starts_with("/usr/bin") {
        if has_arch_release {
            return InstallMethod::Aur;
        }
        if has_distro_package_manager {
            return InstallMethod::SystemPackage;
        }
    }
    InstallMethod::Unknown
}

fn current_install_method() -> InstallMethod {
    let exe_path = env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cargo_home = env::var("CARGO_HOME").ok();
    let has_arch_release = std::path::Path::new("/etc/arch-release").exists();
    let has_distro_package_manager = std::path::Path::new("/etc/debian_version").exists()
        || std::path::Path::new("/etc/redhat-release").exists()
        || std::path::Path::new("/etc/fedora-release").exists();
    detect_install_method(
        &exe_path,
        cargo_home.as_deref(),
        has_arch_release,
        has_distro_package_manager,
    )
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    match (
        semver::Version::parse(latest),
        semver::Version::parse(current),
    ) {
        (Ok(latest), Ok(current)) => latest > current,
        _ => false,
    }
}

// Function to print a customized version info
pub fn print_version_info() {
    let current = env!("CARGO_PKG_VERSION");

    // Only check for updates on an interactive terminal so scripts,
    // pipes, and CI get instant output without network access
    let latest = if io::stdout().is_terminal() {
        fetch_latest_release()
    } else {
        None
    };

    let mut right = vec![
        String::new(),
        format!("Feluda v{current}")
            .bright_white()
            .bold()
            .to_string(),
        "A dependency license checker".bright_yellow().to_string(),
    ];
    match &latest {
        Some(release) if is_newer_version(&release.version, current) => {
            right.push(
                format!("Update available: v{}", release.version)
                    .yellow()
                    .bold()
                    .to_string(),
            );
            right.push(current_install_method().upgrade_line());
        }
        Some(_) => right.push("You're on the latest version ✓".green().to_string()),
        None => {}
    }

    println!("{}", render_banner(&right));

    if let Some(release) = &latest {
        if !release.notes.is_empty() {
            println!();
            println!(
                "  {}",
                format!("What's new in v{}:", release.version)
                    .bright_cyan()
                    .bold()
            );
            for note in &release.notes {
                println!("    {} {}", "•".cyan(), note);
            }
            println!(
                "    {} {}",
                "Full notes:".dimmed(),
                release.url.blue().underline()
            );
        }
    }

    let detail = |label: &str, value: String| {
        println!("  {}{}", format!("{label:<12}").bright_cyan().bold(), value);
    };
    println!();
    detail("License", env!("CARGO_PKG_LICENSE").to_string());
    detail(
        "Repository",
        env!("CARGO_PKG_REPOSITORY").blue().underline().to_string(),
    );
    detail(
        "Docs",
        "https://feluda.readthedocs.io"
            .blue()
            .underline()
            .to_string(),
    );
    println!();
    println!(
        "  {}",
        "Found Feluda useful? ✨ Star the repository!"
            .yellow()
            .bold()
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
        eprint!("\x1B[2K\r");

        // Print initial message with spinner
        eprint!("{} {} ", spinner_frames[0].cyan(), message);
        io::stderr().flush().unwrap();

        let handle = thread::spawn(move || {
            let mut frame_idx = 0;
            while running.load(Ordering::Relaxed) {
                frame_idx = (frame_idx + 1) % spinner_frames.len();

                // Clear the current line and move to beginning
                eprint!("\x1B[2K\r");

                // Print spinner and message
                let spinner_char = spinner_frames[frame_idx];
                eprint!("{} {} ", spinner_char.cyan(), message);

                // Print progress info if available
                if let Some(ref progress_text) = *progress.lock().unwrap() {
                    eprint!("({progress_text})");
                }

                io::stderr().flush().unwrap();
                thread::sleep(Duration::from_millis(80));
            }

            // Clear line and print completion message
            eprint!("\x1B[2K\r");
            eprint!("{} {} ", "✓".green().bold(), message);
            if let Some(ref progress_text) = *progress.lock().unwrap() {
                eprint!("({progress_text})");
            }
            eprintln!(" ✅");
            io::stderr().flush().unwrap();
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
            github_token: None,
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
        assert!(cli.github_token.is_none());
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
            github_token: None,
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
            Commands::Sbom { .. }
            | Commands::Cache { .. }
            | Commands::Init { .. }
            | Commands::Watch { .. } => {
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
            github_token: None,
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
            Commands::Sbom { .. }
            | Commands::Cache { .. }
            | Commands::Init { .. }
            | Commands::Watch { .. } => {
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
        assert!(help_text.contains(&format!("Feluda v{}", env!("CARGO_PKG_VERSION"))));
        assert!(help_text.contains("https://feluda.readthedocs.io"));
        // The Pixelspace wordmark art is present, after a leading blank line
        assert!(help_text.starts_with('\n'));
        assert!(help_text.contains('▌'));
    }

    #[test]
    fn test_render_banner_pads_rows_beyond_art() {
        let banner = render_banner(&[
            String::new(),
            "line one".to_string(),
            "line two".to_string(),
            "line three".to_string(),
            "line four".to_string(),
        ]);
        let lines: Vec<&str> = banner.lines().collect();
        // One leading blank line, then max(art rows, right rows)
        assert_eq!(lines.len(), 6);
        assert!(lines[0].is_empty());
        // The row below the wordmark keeps the right column alignment
        assert!(lines[5].contains("line four"));
        assert!(lines[5].starts_with("  "));
    }

    #[test]
    fn test_extract_release_bullets() {
        let body = "## What's Changed\n\
            * feat(tui): overhaul the table layout by @anistark in https://github.com/anistark/feluda/pull/236\n\
            - fix(java): resolve maven pom properties\n\
            not a bullet\n\
            * \n";
        let bullets = extract_release_bullets(body);
        assert_eq!(
            bullets,
            vec![
                "feat(tui): overhaul the table layout",
                "fix(java): resolve maven pom properties",
            ]
        );
    }

    #[test]
    fn test_extract_release_bullets_caps_at_five() {
        let body = "- one\n- two\n- three\n- four\n- five\n- six\n";
        assert_eq!(extract_release_bullets(body).len(), 5);
    }

    #[test]
    fn test_truncate_chars() {
        assert_eq!(truncate_chars("short", 70), "short");
        let long = "x".repeat(80);
        let truncated = truncate_chars(&long, 70);
        assert_eq!(truncated.chars().count(), 70);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn test_detect_install_method() {
        // Homebrew resolves through the Cellar on macOS, both prefixes
        assert_eq!(
            detect_install_method(
                "/opt/homebrew/Cellar/feluda/1.14.0/bin/feluda",
                None,
                false,
                false
            ),
            InstallMethod::Homebrew
        );
        assert_eq!(
            detect_install_method(
                "/usr/local/Cellar/feluda/1.14.0/bin/feluda",
                None,
                false,
                false
            ),
            InstallMethod::Homebrew
        );
        assert_eq!(
            detect_install_method(
                "/home/linuxbrew/.linuxbrew/Cellar/feluda/1.14.0/bin/feluda",
                None,
                false,
                true
            ),
            InstallMethod::Homebrew
        );
        assert_eq!(
            detect_install_method("/Users/dev/.cargo/bin/feluda", None, false, false),
            InstallMethod::Cargo
        );
        assert_eq!(
            detect_install_method(
                "/custom/rust/bin/feluda",
                Some("/custom/rust"),
                false,
                false
            ),
            InstallMethod::Cargo
        );
        assert_eq!(
            detect_install_method("/usr/bin/feluda", None, true, false),
            InstallMethod::Aur
        );
        assert_eq!(
            detect_install_method("/usr/bin/feluda", None, false, true),
            InstallMethod::SystemPackage
        );
        assert_eq!(
            detect_install_method("/usr/local/bin/feluda", None, false, false),
            InstallMethod::Unknown
        );
    }

    #[test]
    fn test_upgrade_line_per_method() {
        assert!(InstallMethod::Homebrew
            .upgrade_line()
            .contains("brew upgrade feluda"));
        assert!(InstallMethod::Cargo
            .upgrade_line()
            .contains("cargo install feluda"));
        assert!(InstallMethod::Aur.upgrade_line().contains("paru -S feluda"));
        assert!(InstallMethod::SystemPackage
            .upgrade_line()
            .contains("releases page"));
        assert!(InstallMethod::Unknown
            .upgrade_line()
            .contains("cargo install feluda"));
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("1.15.0", "1.14.0"));
        assert!(is_newer_version("2.0.0", "1.99.9"));
        assert!(!is_newer_version("1.14.0", "1.14.0"));
        assert!(!is_newer_version("1.13.2", "1.14.0"));
        assert!(!is_newer_version("not-a-version", "1.14.0"));
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
