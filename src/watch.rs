//! `feluda watch` — continuous license compliance.
//!
//! Watches the project tree for filesystem changes and re-runs the license scan
//! whenever a dependency descriptor changes. The set of files that count as a
//! dependency change is defined by [`crate::manifest`], the same source of truth
//! the scanner uses to recognise project files.
//!
//! Watch mode is report-only: it never opens the interactive TUI (`--gui`) and
//! never exits on restrictive/incompatible findings — it just keeps reporting
//! until interrupted (Ctrl-C).

use crate::debug::{log, FeludaError, FeludaResult, LogLevel};
use crate::manifest;
use crate::{analyze_dependencies, annotate_compatibility, report_analysis, CheckConfig};
use colored::Colorize;
use notify::{Event, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

/// Run a single scan-and-report pass without touching the process exit code.
///
/// Errors are logged and swallowed so a transient parse failure (e.g. an editor
/// writing a half-finished manifest) doesn't tear down the watch session.
fn scan_once(config: &CheckConfig) {
    match analyze_dependencies(config) {
        Ok((mut analyzed_data, project_license)) => {
            if analyzed_data.is_empty() {
                log(LogLevel::Warn, "No dependencies found to analyze.");
                return;
            }
            annotate_compatibility(&mut analyzed_data, &project_license, config.strict);
            let _ = report_analysis(analyzed_data, project_license, config);
        }
        Err(e) => {
            // Keep watching even if this pass failed.
            e.log();
        }
    }
}

/// Whether a batch of filesystem events touches any dependency descriptor.
fn event_touches_dependency(result: &notify::Result<Event>) -> bool {
    match result {
        Ok(event) => event.paths.iter().any(|p| manifest::is_relevant_change(p)),
        Err(_) => false,
    }
}

/// Entry point for the `watch` subcommand.
///
/// `config.gui` is expected to be `false`; the caller rejects `--gui` before we
/// get here.
pub fn handle_watch_command(config: CheckConfig, debounce_ms: u64) -> FeludaResult<()> {
    let path = config.path.clone();
    let root = Path::new(&path);

    if !root.exists() {
        eprintln!("❌ Watch path does not exist: {path}");
        return Err(FeludaError::InvalidData(format!(
            "Watch path does not exist: {path}"
        )));
    }

    log(
        LogLevel::Info,
        &format!("Starting watch mode on path: {path}"),
    );

    // Initial scan so the user sees the current state immediately.
    scan_once(&config);

    let watched = manifest::discover_dependency_files(root);
    println!(
        "\n{} {}",
        "👁  Watching".bright_cyan().bold(),
        format!(
            "{} ({} dependency file{} tracked) — press Ctrl-C to stop",
            path,
            watched.len(),
            if watched.len() == 1 { "" } else { "s" }
        )
        .bright_cyan()
    );
    if watched.is_empty() {
        println!(
            "{}",
            "⚠  No dependency files found yet; will react when one appears.".yellow()
        );
    }

    // Filesystem watcher. Events are delivered to the channel; we debounce bursts
    // (editors emit several events per save) before re-scanning.
    let (tx, rx) = channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| FeludaError::InvalidData(format!("Failed to initialize file watcher: {e}")))?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| FeludaError::InvalidData(format!("Failed to watch {path}: {e}")))?;

    let debounce = Duration::from_millis(debounce_ms);

    loop {
        // Block until the next event arrives (or the watcher goes away).
        let first = match rx.recv() {
            Ok(event) => event,
            Err(_) => {
                log(LogLevel::Warn, "Watch channel closed, stopping watch mode");
                break;
            }
        };

        if !event_touches_dependency(&first) {
            continue;
        }

        // Debounce: drain any further events that arrive within the window so a
        // single save (or a burst of related changes) triggers exactly one scan.
        let deadline = Instant::now() + debounce;
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(remaining) {
                Ok(_) => continue,
                Err(_) => break,
            }
        }

        println!(
            "\n{} {}",
            "🔄".bright_yellow(),
            "Dependency change detected — re-scanning…"
                .bright_yellow()
                .bold()
        );
        scan_once(&config);
    }

    Ok(())
}
