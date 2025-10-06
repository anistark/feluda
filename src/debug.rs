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
pub fn log(level: LogLevel, message: &str) {
    if is_debug_mode() {
        println!("[{}] {}", level.as_colored_str(), message);
    }
}

/// Log an error with context information if debug mode is enabled
pub fn log_error<E: std::fmt::Display>(context: &str, error: &E) {
    if is_debug_mode() {
        println!(
            "[{}] {}: {}",
            LogLevel::Error.as_colored_str(),
            context,
            error
        );
    }
}

/// Log detailed information about a value if debug mode is enabled
pub fn log_debug<T: std::fmt::Debug + ?Sized>(context: &str, value: &T) {
    if is_debug_mode() {
        println!(
            "[{}] {}: {:?}",
            LogLevel::Trace.as_colored_str(),
            context,
            value
        );
    }
}

/// Conditionally execute a function and log the result if debug mode is enabled
#[allow(dead_code)]
pub fn with_debug<F, T>(context: &str, f: F) -> T
where
    F: FnOnce() -> T,
    T: std::fmt::Debug,
{
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
        log_debug(context, &result);
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

    #[error("Repository clone error: {0}")]
    RepositoryClone(String),

    #[error("Temporary directory error: {0}")]
    TempDir(String),

    #[error("TUI initialization error: {0}")]
    TuiInit(String),

    #[error("TUI runtime error: {0}")]
    TuiRuntime(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("File write error: {0}")]
    FileWrite(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Unknown error: {0}")]
    #[allow(dead_code)]
    Unknown(String),
}

impl FeludaError {
    pub fn log(&self) {
        log_error("Error occurred", self);
    }
}

/// Result type alias for Feluda operations
pub type FeludaResult<T> = Result<T, FeludaError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_mode_toggle() {
        // Start with debug off
        set_debug_mode(false);
        assert!(!is_debug_mode());

        // Turn on debug mode
        set_debug_mode(true);
        assert!(is_debug_mode());

        // Turn off debug mode
        set_debug_mode(false);
        assert!(!is_debug_mode());
    }

    #[test]
    fn test_log_level_as_str() {
        assert_eq!(LogLevel::Info.as_str(), "INFO");
        assert_eq!(LogLevel::Warn.as_str(), "WARN");
        assert_eq!(LogLevel::Error.as_str(), "ERROR");
        assert_eq!(LogLevel::Trace.as_str(), "TRACE");
    }

    #[test]
    fn test_log_level_equality() {
        assert_eq!(LogLevel::Info, LogLevel::Info);
        assert_eq!(LogLevel::Warn, LogLevel::Warn);
        assert_eq!(LogLevel::Error, LogLevel::Error);
        assert_eq!(LogLevel::Trace, LogLevel::Trace);

        assert_ne!(LogLevel::Info, LogLevel::Warn);
        assert_ne!(LogLevel::Error, LogLevel::Trace);
    }

    #[test]
    fn test_feluda_error_variants() {
        let io_error = FeludaError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found",
        ));
        let config_error = FeludaError::Config("Invalid config".to_string());
        let parser_error = FeludaError::Parser("Parse failed".to_string());
        let license_error = FeludaError::License("License error".to_string());
        let unknown_error = FeludaError::Unknown("Unknown issue".to_string());

        assert!(io_error.to_string().contains("IO error"));
        assert!(config_error.to_string().contains("Configuration error"));
        assert!(parser_error.to_string().contains("Parser error"));
        assert!(license_error.to_string().contains("License analysis error"));
        assert!(unknown_error.to_string().contains("Unknown error"));
    }

    #[test]
    fn test_feluda_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access denied");
        let feluda_err: FeludaError = io_err.into();

        match feluda_err {
            FeludaError::Io(_) => {}
            _ => panic!("Expected IO error variant"),
        }
    }

    #[test]
    fn test_feluda_error_from_reqwest() {
        let client = reqwest::blocking::Client::new();
        let reqwest_err = client
            .get("http://invalid-url-that-does-not-exist.local")
            .send()
            .unwrap_err();
        let feluda_err: FeludaError = reqwest_err.into();

        match feluda_err {
            FeludaError::Http(_) => {} // Expected
            _ => panic!("Expected HTTP error variant"),
        }
    }

    #[test]
    fn test_with_debug_function() {
        set_debug_mode(true);

        let result = with_debug("Test operation", || {
            std::thread::sleep(std::time::Duration::from_millis(1));
            "completed"
        });

        assert_eq!(result, "completed");

        set_debug_mode(false);
    }

    #[test]
    fn test_with_debug_function_disabled() {
        set_debug_mode(false);

        let result = with_debug("Test operation", || "completed without debug");

        assert_eq!(result, "completed without debug");
    }

    #[test]
    fn test_log_functions_when_debug_disabled() {
        set_debug_mode(false);

        log(LogLevel::Info, "Test message");
        log(LogLevel::Warn, "Test warning");
        log(LogLevel::Error, "Test error");
        log(LogLevel::Trace, "Test trace");

        log_error("Test context", &"Test error");
        log_debug("Test context", &"Test value");
    }

    #[test]
    fn test_log_functions_when_debug_enabled() {
        set_debug_mode(true);

        log(LogLevel::Info, "Test message");
        log(LogLevel::Warn, "Test warning");
        log(LogLevel::Error, "Test error");
        log(LogLevel::Trace, "Test trace");

        log_error("Test context", &"Test error");
        log_debug("Test context", &vec![1, 2, 3]);

        set_debug_mode(false);
    }

    #[test]
    fn test_feluda_error_log() {
        set_debug_mode(true);

        let error = FeludaError::Config("Test error".to_string());
        error.log();

        let error2 = FeludaError::Unknown("Another test".to_string());
        error2.log();

        set_debug_mode(false);
    }

    #[test]
    fn test_feluda_result_alias() {
        fn test_function() -> FeludaResult<String> {
            Ok("success".to_string())
        }

        let result = test_function();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[test]
    fn test_feluda_result_error() {
        fn test_function() -> FeludaResult<String> {
            Err(FeludaError::Config("Test failure".to_string()))
        }

        let result = test_function();
        assert!(result.is_err());
        match result.unwrap_err() {
            FeludaError::Config(msg) => assert_eq!(msg, "Test failure"),
            _ => panic!("Expected Config error"),
        }
    }

    #[test]
    fn test_log_level_debug_format() {
        let info = LogLevel::Info;
        let debug_str = format!("{info:?}");
        assert_eq!(debug_str, "Info");
    }

    #[test]
    fn test_feluda_error_debug_format() {
        let error = FeludaError::Config("test config error".to_string());
        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("Config"));
        assert!(debug_str.contains("test config error"));
    }

    #[test]
    fn test_multiple_debug_contexts() {
        set_debug_mode(true);

        let result1 = with_debug("First operation", || "result1");
        let result2 = with_debug("Second operation", || "result2");

        assert_eq!(result1, "result1");
        assert_eq!(result2, "result2");

        set_debug_mode(false);
    }

    #[test]
    fn test_log_with_special_characters() {
        set_debug_mode(true);

        log(
            LogLevel::Info,
            "Message with unicode: 🚀 and newlines\nand tabs\t",
        );
        log_debug("Context with symbols", &"Special chars: !@#$%^&*()");

        set_debug_mode(false);
    }
}
