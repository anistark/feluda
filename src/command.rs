//! Every external command feluda runs, each with a deadline.
//!
//! `Command::output` waits as long as the child takes, so one `dotnet list package` stuck on a
//! restore, or one `npm view` stuck on a registry, used to hold the whole scan. Call sites use
//! [`list`] or [`lookup`] in its place:
//!
//! - [`list`] is for a command that produces the dependency list (`cargo metadata`,
//!   `go mod graph`, `dotnet list package`, `uv lock`, `npm ls`). It gets `[timeouts] command`.
//!   When one is stopped, one line on stderr says so and it is not run again for the rest of the
//!   run: the Node analyzer tries several `pnpm list` forms in turn, and a `go.work` runs
//!   `go mod graph` once per member, so retrying a command that hung would cost the deadline again
//!   each time.
//! - [`lookup`] is for a command asked about one package (`npm view`, `pkg-config`, `dpkg-query`)
//!   or a one-off query (`go env`, `Rscript`). It stands in for a registry request, so it gets
//!   `[timeouts] request`. After [`GIVE_UP_AFTER`] timeouts in a row it is not run again for the
//!   rest of the run, as `http_stats` does with a host that stops answering.
//! - [`interactive`] is for a command the user watches and may answer, like `sudo dpkg -i` or a
//!   `cargo install` compiling for minutes. It inherits the terminal and has no deadline.
//!
//! A command stopped at its deadline returns an [`io::ErrorKind::TimedOut`] error and whatever it
//! printed is discarded, so a call site takes the fallback it already has for a command that fails
//! (direct dependencies for Go, .NET and uv) and never mistakes half a graph for the whole one.
//!
//! Only the direct child is killed. A grandchild that inherited its pipes (npm and dotnet start
//! helpers) is not waited for past the deadline: the threads reading the pipes are left to finish
//! on their own, so the scan carries on, and Ctrl-C still reaches every process since none is moved to a group of
//! its own.

use std::collections::HashMap;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::timeouts;
use crate::debug::{log, LogLevel};

/// Timeouts in a row after which a lookup command is not run again for the rest of the run.
pub const GIVE_UP_AFTER: u32 = 3;

/// Longest pause between two checks on a running child.
const MAX_POLL: Duration = Duration::from_millis(50);

/// Run a command that produces a dependency list, with `[timeouts] command` as its deadline.
///
/// `label` names it in the stderr line, as a user would type it: `"go mod graph"`.
pub fn list(label: &str, command: &mut Command) -> io::Result<Output> {
    counted(
        label,
        command,
        Kind::List,
        Duration::from_secs(timeouts().command),
    )
}

/// Run a command asked about one package, with `[timeouts] request` as its deadline.
///
/// `label` is how the command is named in messages and what timeouts are counted against, so it
/// names the subcommand and not the package: `"npm view"`, not `"npm view left-pad"`.
pub fn lookup(label: &str, command: &mut Command) -> io::Result<Output> {
    counted(
        label,
        command,
        Kind::Lookup,
        Duration::from_secs(timeouts().request),
    )
}

/// Run a command in the user's terminal and wait for as long as it takes.
pub fn interactive(command: &mut Command) -> io::Result<ExitStatus> {
    command.status()
}

#[derive(Clone, Copy)]
enum Kind {
    List,
    Lookup,
}

impl Kind {
    /// Timeouts in a row after which a command of this kind is not run again.
    fn give_up_after(self) -> u32 {
        match self {
            Kind::List => 1,
            Kind::Lookup => GIVE_UP_AFTER,
        }
    }
}

fn counted(label: &str, command: &mut Command, kind: Kind, limit: Duration) -> io::Result<Output> {
    if given_up(label, kind) {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("{label} skipped: it timed out earlier in this run"),
        ));
    }
    let result = run(command, limit);
    note(label, kind, limit, is_timeout(&result));
    result.map_err(|err| labelled(label, err))
}

fn is_timeout(result: &io::Result<Output>) -> bool {
    matches!(result, Err(err) if err.kind() == io::ErrorKind::TimedOut)
}

/// Prefix an error with the command it came from, keeping its kind.
fn labelled(label: &str, err: io::Error) -> io::Error {
    io::Error::new(err.kind(), format!("{label}: {err}"))
}

/// Spawn `command`, collect its output, and kill it if it is still running after `limit`.
fn run(command: &mut Command, limit: Duration) -> io::Result<Output> {
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = read_on_thread(child.stdout.take());
    let stderr = read_on_thread(child.stderr.take());

    let status = match wait_until(&mut child, started + limit)? {
        Some(status) => status,
        None => return Err(stop(&mut child, limit)),
    };

    // The child has exited, but a grandchild may still hold its pipes open. Wait for them only as
    // long as the deadline allows.
    let stdout = collect(stdout, started + limit).ok_or_else(|| timed_out(limit))?;
    let stderr = collect(stderr, started + limit).ok_or_else(|| timed_out(limit))?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Poll the child until it exits or `deadline` passes, backing off from 1 ms to [`MAX_POLL`].
fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
    let mut pause = Duration::from_millis(1);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(MAX_POLL);
    }
}

fn stop(child: &mut Child, limit: Duration) -> io::Error {
    if let Err(err) = child.kill() {
        log(
            LogLevel::Warn,
            &format!("Could not stop a command past its deadline: {err}"),
        );
    }
    // Reap it, so no zombie is left behind. Killing the direct child makes this return at once.
    let _ = child.wait();
    timed_out(limit)
}

fn timed_out(limit: Duration) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!("did not finish within {} s", limit.as_secs_f64()),
    )
}

/// Read a pipe to its end on a thread of its own, so a child writing a lot to both pipes never
/// blocks on the one nobody is reading.
fn read_on_thread(pipe: Option<impl Read + Send + 'static>) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    match pipe {
        Some(mut pipe) => {
            thread::spawn(move || {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer);
                let _ = sender.send(buffer);
            });
        }
        None => {
            let _ = sender.send(Vec::new());
        }
    }
    receiver
}

/// What a pipe reader collected, or `None` if the pipe is still open at `deadline`.
fn collect(receiver: Receiver<Vec<u8>>, deadline: Instant) -> Option<Vec<u8>> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    match receiver.recv_timeout(remaining) {
        Ok(buffer) => Some(buffer),
        Err(RecvTimeoutError::Disconnected) => Some(Vec::new()),
        Err(RecvTimeoutError::Timeout) => None,
    }
}

/// Timeouts in a row, per label. Reaching the kind's limit is final for the run.
static TIMEOUTS_IN_A_ROW: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(Default::default);

fn given_up(label: &str, kind: Kind) -> bool {
    TIMEOUTS_IN_A_ROW
        .lock()
        .map(|counts| counts.get(label).copied().unwrap_or(0) >= kind.give_up_after())
        .unwrap_or(false)
}

/// Count a timeout against the label, or clear its count when the command finished (successfully
/// or not), and say once on stderr when the label is given up on.
fn note(label: &str, kind: Kind, limit: Duration, timed_out: bool) {
    let Ok(mut counts) = TIMEOUTS_IN_A_ROW.lock() else {
        return;
    };
    let count = counts.entry(label.to_owned()).or_default();
    if *count >= kind.give_up_after() {
        return;
    }
    if !timed_out {
        *count = 0;
        return;
    }
    *count += 1;
    if *count < kind.give_up_after() {
        return;
    }
    match kind {
        Kind::List => eprintln!(
            "⚠️ `{label}` did not finish within {} s and was stopped; the report falls back to what feluda can read without it. Raise `[timeouts] command` to wait longer.",
            limit.as_secs()
        ),
        Kind::Lookup => eprintln!(
            "⚠️ `{label}` timed out {GIVE_UP_AFTER} times in a row; not running it again for the rest of this run, so feluda answers from its other sources instead."
        ),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn output_and_status_come_back_as_from_command_output() {
        let output = run(
            &mut sh("echo out; echo err >&2; exit 3"),
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(output.stdout, b"out\n");
        assert_eq!(output.stderr, b"err\n");
        assert_eq!(output.status.code(), Some(3));
    }

    #[test]
    fn a_command_past_its_deadline_is_stopped() {
        let started = Instant::now();
        let err = run(
            &mut sh("echo partial; sleep 30"),
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_grandchild_holding_the_pipes_does_not_hold_the_scan() {
        // The shell exits at once, its background `sleep` keeps stdout open.
        let started = Instant::now();
        let err = run(&mut sh("sleep 30 & echo done"), Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn output_larger_than_a_pipe_buffer_does_not_deadlock() {
        // 1 MiB on each pipe, far past the 64 KiB a pipe holds before the writer blocks.
        let output = run(
            &mut sh("head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2"),
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(output.stdout.len(), 1 << 20);
        assert_eq!(output.stderr.len(), 1 << 20);
    }

    #[test]
    fn a_missing_program_is_an_error_but_not_a_timeout() {
        let err = run(
            &mut Command::new("feluda-no-such-program"),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_lookup_that_keeps_timing_out_is_given_up_on() {
        let label = "test hanging lookup";
        for _ in 0..GIVE_UP_AFTER {
            let err = counted(
                label,
                &mut sh("sleep 30"),
                Kind::Lookup,
                Duration::from_millis(100),
            )
            .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        }
        // Would succeed if it ran; it must not run at all.
        let err = counted(
            label,
            &mut sh("exit 0"),
            Kind::Lookup,
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(err.to_string().contains("skipped"), "{err}");
    }

    #[test]
    fn a_lookup_that_finishes_resets_the_count() {
        let label = "test flaky lookup";
        let hang = || {
            counted(
                label,
                &mut sh("sleep 30"),
                Kind::Lookup,
                Duration::from_millis(100),
            )
        };
        for _ in 0..GIVE_UP_AFTER - 1 {
            assert!(hang().is_err());
        }
        // A failing exit status is still an answer.
        let output = counted(
            label,
            &mut sh("exit 1"),
            Kind::Lookup,
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(1));
        for _ in 0..GIVE_UP_AFTER - 1 {
            assert!(hang().is_err());
        }
        assert!(!given_up(label, Kind::Lookup));
    }

    #[test]
    fn a_listing_command_that_timed_out_is_not_run_again() {
        let label = "test hanging listing";
        let err = counted(
            label,
            &mut sh("sleep 30"),
            Kind::List,
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        let err = counted(
            label,
            &mut sh("exit 0"),
            Kind::List,
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert!(err.to_string().contains("skipped"), "{err}");
    }

    #[test]
    fn errors_name_the_command_and_keep_their_kind() {
        let err = labelled("go mod graph", timed_out(Duration::from_secs(300)));
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert_eq!(err.to_string(), "go mod graph: did not finish within 300 s");
    }
}
