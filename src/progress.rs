use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::{self, Write};
use std::thread;
use std::time::Duration;
use colored::*;

/// TODO: Global progress tracker for coordinating multiple concurrent operations.
/// Will be used when implementing support for analyzing multiple root projects
/// simultaneously with per-project progress indicators.
#[allow(dead_code)]
pub struct ProgressTracker {
    #[allow(dead_code)]
    total: usize,
    completed: Arc<AtomicUsize>,
    #[allow(dead_code)]
    current_task: Arc<Mutex<String>>,
    running: Arc<Mutex<bool>>,
    handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl ProgressTracker {
    /// TODO: Create a new progress tracker. Will be used when implementing
    /// multi-project analysis mode with detailed progress tracking per project.
    #[allow(dead_code)]
    pub fn new(total: usize) -> Self {
        Self {
            total,
            completed: Arc::new(AtomicUsize::new(0)),
            current_task: Arc::new(Mutex::new(String::new())),
            running: Arc::new(Mutex::new(false)),
            handle: Arc::new(Mutex::new(None)),
        }
    }

    /// TODO: Start the progress indicator thread. Will be used for displaying
    /// concurrent progress updates when analyzing multiple projects in parallel.
    #[allow(dead_code)]
    pub fn start(&self) {
        let total = self.total;

        *self.running.lock().unwrap() = true;

        let spinner_frames = vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let completed_for_thread = Arc::clone(&self.completed);
        let current_task_for_thread = Arc::clone(&self.current_task);
        let running_for_thread = Arc::clone(&self.running);

        let handle = thread::spawn(move || {
            let mut frame_idx = 0;
            while *running_for_thread.lock().unwrap() {
                frame_idx = (frame_idx + 1) % spinner_frames.len();

                let completed_count = completed_for_thread.load(Ordering::Relaxed);
                let current = current_task_for_thread.lock().unwrap().clone();

                // Clear line and show progress
                print!("\x1B[2K\r");
                let spinner = spinner_frames[frame_idx].cyan();
                let progress_text = format!("[{}/{}]", completed_count, total);

                print!("{} {} ", spinner, "Analyzing projects".bright_white().bold());
                print!("{} ", progress_text.bright_cyan());

                if !current.is_empty() {
                    print!("({})", current.yellow());
                }

                io::stdout().flush().unwrap();
                thread::sleep(Duration::from_millis(80));
            }

            // Final message
            print!("\x1B[2K\r");
            println!(
                "{} {} {} {}",
                "✓".green().bold(),
                "Analyzed".bright_white().bold(),
                format!("{} projects", total).bright_cyan().bold(),
                "✅"
            );
            io::stdout().flush().unwrap();
        });

        if let Ok(mut h) = self.handle.lock() {
            *h = Some(handle);
        }
    }

    /// TODO: Update the current task being worked on. Will be used to display
    /// which specific project or analysis step is currently executing.
    #[allow(dead_code)]
    pub fn set_current_task(&self, task: impl Into<String>) {
        if let Ok(mut guard) = self.current_task.lock() {
            *guard = task.into();
        }
    }

    /// TODO: Mark a task as completed. Will be called to update progress counters
    /// as each project analysis completes in multi-project scenarios.
    #[allow(dead_code)]
    pub fn inc_completed(&self) {
        self.completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Stop the progress indicator
    pub fn stop(&self) {
        if let Ok(mut guard) = self.running.lock() {
            *guard = false;
        }
        if let Ok(mut h) = self.handle.lock() {
            if let Some(handle) = h.take() {
                let _ = handle.join();
            }
        }
    }

    /// Get the current completion count
    #[allow(dead_code)]
    pub fn get_completed(&self) -> usize {
        self.completed.load(Ordering::Relaxed)
    }
}

impl Drop for ProgressTracker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_tracker_creation() {
        let tracker = ProgressTracker::new(10);
        assert_eq!(tracker.total, 10);
        assert_eq!(tracker.completed.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_progress_tracker_increment() {
        let tracker = ProgressTracker::new(5);
        tracker.inc_completed();
        tracker.inc_completed();
        assert_eq!(tracker.get_completed(), 2);
    }

    #[test]
    fn test_progress_tracker_set_task() {
        let tracker = ProgressTracker::new(1);
        tracker.set_current_task("test task");
        let task = tracker.current_task.lock().unwrap().clone();
        assert_eq!(task, "test task");
    }
}
