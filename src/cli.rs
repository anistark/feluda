use clap::{ArgGroup, Parser, ValueEnum};
use colored::*;
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

#[derive(Parser, Debug)]
#[command(author, version)]
#[command(about = env!("CARGO_PKG_DESCRIPTION"))]
#[command(
    long_about = "Feluda is a CLI tool that analyzes the dependencies of a project, identifies their licenses, and flags any that may restrict personal or commercial usage."
)]
#[command(group(ArgGroup::new("output").args(["json"])))]
#[command(before_help = format_before_help())]
pub struct Cli {
    /// Path to the local project directory
    #[arg(short, long, default_value = "./")]
    pub path: String,

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

    /// Show only restrictive dependencies in strict mode
    #[arg(long, short)]
    pub strict: bool,

    /// Enable TUI table
    #[arg(long, short)]
    pub gui: bool,

    /// Enable debug mode
    #[arg(long, short)]
    pub debug: bool,

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
    let title = format!("Feluda v{}", version);
    let width = title.len() + 4;
    let border = "─".repeat(width);

    println!("{}", format!("┌{}┐", border).bright_red());
    println!(
        "{}",
        format!("│ {}   │", title.bright_white().bold()).bright_red()
    );
    println!("{}", format!("└{}┘", border).bright_red());
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

        // Print initial message with spinner
        print!("{} {} ", spinner_frames[0].cyan(), message);
        io::stdout().flush().unwrap();

        let handle = thread::spawn(move || {
            let mut frame_idx = 0;
            while running.load(Ordering::Relaxed) {
                frame_idx = (frame_idx + 1) % spinner_frames.len();

                // Clear the current line and move to beginning
                print!("\r");

                // Print spinner and message
                let spinner_char = spinner_frames[frame_idx];
                print!("{} {} ", spinner_char.cyan(), message);

                // Print progress info if available
                if let Some(ref progress_text) = *progress.lock().unwrap() {
                    print!("({})", progress_text);
                }

                io::stdout().flush().unwrap();
                thread::sleep(Duration::from_millis(80));
            }

            // Clear line and print completion message
            print!("\r");
            print!("{} {} ", "✓".green().bold(), message);
            if let Some(ref progress_text) = *progress.lock().unwrap() {
                print!("({})", progress_text);
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
        log(LogLevel::Info, &format!("Operation: {}", message));
        let start = std::time::Instant::now();
        let indicator = LoadingIndicator::new(message);
        let result = f(&indicator);
        let duration = start.elapsed();
        log(LogLevel::Info, &format!("Completed in {:?}", duration));
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
}
