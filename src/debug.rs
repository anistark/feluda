use std::sync::atomic::{AtomicBool, Ordering};

// Static atomic flag for debug mode
pub static DEBUG_MODE: AtomicBool = AtomicBool::new(false);

// Log levels for different types of debug information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Trace,
}

impl LogLevel {
    // We're keeping this function since it's needed for the Debug implementation
    #[allow(dead_code)]
    fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Trace => "TRACE",
        }
    }

    fn as_colored_str(&self) -> colored::ColoredString {
        use colored::*;
        match self {
            LogLevel::Info => "INFO".green(),
            LogLevel::Warn => "WARN".yellow(),
            LogLevel::Error => "ERROR".red(),
            LogLevel::Trace => "TRACE".blue(),
        }
    }
}

/// Set the debug mode flag
pub fn set_debug_mode(debug: bool) {
    DEBUG_MODE.store(debug, Ordering::Relaxed);
    if debug {
        log(LogLevel::Info, "Debug mode enabled");
    }
}

/// Check if debug mode is enabled
pub fn is_debug_mode() -> bool {
    DEBUG_MODE.load(Ordering::Relaxed)
}

/// Log a message with the specified level if debug mode is enabled
pub fn log(level: LogLevel, message: impl Into<String>) {
    if is_debug_mode() {
        println!("[{}] {}", level.as_colored_str(), message.into());
    }
}

/// Log an error with context information if debug mode is enabled
pub fn log_error<E: std::fmt::Display>(context: impl Into<String>, error: &E) {
    if is_debug_mode() {
        println!(
            "[{}] {}: {}",
            LogLevel::Error.as_colored_str(),
            context.into(),
            error
        );
    }
}

/// Log detailed information about a value if debug mode is enabled
pub fn log_debug<T: std::fmt::Debug>(context: impl Into<String>, value: &T) {
    if is_debug_mode() {
        println!(
            "[{}] {}: {:?}",
            LogLevel::Trace.as_colored_str(),
            context.into(),
            value
        );
    }
}

/// Conditionally execute a function and log the result if debug mode is enabled
#[allow(dead_code)]
pub fn with_debug<F, T>(context: impl Into<String>, f: F) -> T
where
    F: FnOnce() -> T,
    T: std::fmt::Debug,
{
    let context = context.into();
    if is_debug_mode() {
        let start = std::time::Instant::now();
        let result = f();
        let duration = start.elapsed();
        println!(
            "[{}] {} completed in {:?}",
            LogLevel::Info.as_colored_str(),
            context,
            duration
        );
        log_debug(&context, &result);
        result
    } else {
        f()
    }
}

/// Create a custom error type that includes debug information
#[derive(Debug, thiserror::Error)]
pub enum FeludaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("License analysis error: {0}")]
    #[allow(dead_code)]
    License(String),

    #[error("Parser error: {0}")]
    Parser(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl FeludaError {
    pub fn log(&self) {
        log_error("Error occurred", self);
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn parser(message: impl Into<String>) -> Self {
        Self::Parser(message.into())
    }

    pub fn unknown(message: impl Into<String>) -> Self {
        Self::Unknown(message.into())
    }
}

/// Result type alias for Feluda operations
pub type FeludaResult<T> = Result<T, FeludaError>;
