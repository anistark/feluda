use crate::debug::{FeludaError, FeludaResult};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub message: String,
    pub field: Option<String>,
    pub line: Option<usize>,
}

impl ValidationIssue {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Error,
            message: message.into(),
            field: None,
            line: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Warning,
            message: message.into(),
            field: None,
            line: None,
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: IssueSeverity::Info,
            message: message.into(),
            field: None,
            line: None,
        }
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub sbom_type: String,
    pub is_valid: bool,
    pub issues: Vec<ValidationIssue>,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
}

impl ValidationReport {
    pub fn new(sbom_type: impl Into<String>) -> Self {
        Self {
            sbom_type: sbom_type.into(),
            is_valid: true,
            issues: Vec::new(),
            error_count: 0,
            warning_count: 0,
            info_count: 0,
        }
    }

    pub fn add_issue(&mut self, issue: ValidationIssue) {
        match issue.severity {
            IssueSeverity::Error => {
                self.error_count += 1;
                self.is_valid = false;
            }
            IssueSeverity::Warning => self.warning_count += 1,
            IssueSeverity::Info => self.info_count += 1,
        }
        self.issues.push(issue);
    }

    pub fn write_output(&self, json: bool, output: Option<String>) -> FeludaResult<()> {
        let output_string = if json {
            serde_json::to_string_pretty(&self).map_err(|e| {
                FeludaError::Serialization(format!("Failed to serialize report: {e}"))
            })?
        } else {
            self.format_text()
        };

        if let Some(path) = output {
            fs::write(&path, &output_string).map_err(|e| {
                FeludaError::FileWrite(format!("Failed to write report to {path}: {e}"))
            })?;
            println!("Report written to: {path}");
        } else {
            println!("{output_string}");
        }

        Ok(())
    }

    fn format_text(&self) -> String {
        use owo_colors::OwoColorize;

        let mut output = String::new();
        output.push_str(&format!("\n{}\n", "━".repeat(60)).bold().to_string());
        let status_icon = if self.is_valid {
            format!("{}", "✓".green())
        } else {
            format!("{}", "✗".red())
        };
        output.push_str(&format!(
            "{} {} SBOM Validation Report\n",
            "".bold(),
            status_icon
        ));
        output.push_str(&format!("{}\n", "━".repeat(60)).bold().to_string());

        output.push_str(&format!("SBOM Type: {}\n", self.sbom_type.bright_cyan()));
        output.push_str(&format!(
            "Status: {}\n",
            if self.is_valid {
                "VALID".green().to_string()
            } else {
                "INVALID".red().to_string()
            }
        ));

        output.push_str("\nIssues Summary:\n");
        output.push_str(&format!(
            "  Errors:   {}\n",
            if self.error_count > 0 {
                format!("{}", self.error_count).red().to_string()
            } else {
                format!("{}", self.error_count).green().to_string()
            }
        ));
        output.push_str(&format!(
            "  Warnings: {}\n",
            if self.warning_count > 0 {
                format!("{}", self.warning_count).yellow().to_string()
            } else {
                format!("{}", self.warning_count).green().to_string()
            }
        ));
        output.push_str(&format!("  Info:     {}\n", self.info_count.bright_blue()));

        if !self.issues.is_empty() {
            output.push_str("\nDetailed Issues:\n");
            output.push_str(&format!("{}\n", "─".repeat(60)));

            for issue in &self.issues {
                let severity_str = match issue.severity {
                    IssueSeverity::Error => "[ERROR]".red().to_string(),
                    IssueSeverity::Warning => "[WARN]".yellow().to_string(),
                    IssueSeverity::Info => "[INFO]".blue().to_string(),
                };

                output.push_str(&format!("{severity_str} {}\n", issue.message));

                if let Some(ref field) = issue.field {
                    output.push_str(&format!("        Field: {}\n", field.bright_black()));
                }

                if let Some(line) = issue.line {
                    output.push_str(&format!("        Line: {line}\n"));
                }
            }

            output.push_str(&format!("{}\n", "─".repeat(60)));
        }

        output
    }
}
