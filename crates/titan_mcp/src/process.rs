//! Ownership and bounded lifecycle operations for a locally configured game.
//!
//! Commands are trusted configuration, never tool arguments. Each command is an
//! executable followed by literal arguments (no shell expansion). Unix commands
//! get a new process group; Windows commands start suspended into a Job Object.
//! Windows requests graceful shutdown via `taskkill /T`, then terminates the job.
//! Ordinary EOF/errors clean up via `Drop`; SIGKILL/power loss cannot run cleanup.

use crate::client::Client;
use alloc::{collections::VecDeque, sync::Arc};
use serde::Serialize;
use std::{
    io::Read,
    path::PathBuf,
    process::{ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
type Child = Box<dyn process_wrap::std::ChildWrapper>;
#[cfg(not(windows))]
use std::process::Child;

fn child_id(child: &Child) -> u32 {
    child.id()
}

// Only JobObject wraps the Windows native Child. Poll/wait its immediate
// inner layer without draining job completion notifications or invoking the
// wrapper's group-wide blocking wait. The job is retained until after cleanup.
fn wait_child(child: &mut Child) -> std::io::Result<ExitStatus> {
    #[cfg(windows)]
    {
        child.inner_mut().wait()
    }
    #[cfg(not(windows))]
    {
        child.wait()
    }
}

fn take_stdout(child: &mut Child) -> Option<ChildStdout> {
    #[cfg(windows)]
    {
        child.stdout().take()
    }
    #[cfg(not(windows))]
    {
        child.stdout.take()
    }
}

fn take_stderr(child: &mut Child) -> Option<ChildStderr> {
    #[cfg(windows)]
    {
        child.stderr().take()
    }
    #[cfg(not(windows))]
    {
        child.stderr.take()
    }
}

fn spawn(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(windows)]
    {
        use process_wrap::std::{CommandWrap, JobObject};
        // This implementation cleans up suspended children AND owned handles
        // if job assignment or thread resumption fails, before returning Err.
        let mut wrapper = CommandWrap::from(std::mem::replace(command, Command::new("")));
        wrapper.wrap(JobObject).spawn()
    }
    #[cfg(not(windows))]
    {
        command.spawn()
    }
}

// Unix waitid(NOWAIT) reserves the leader PID even after exit. Every caller
// signals the group BEFORE the sole wait() that reaps it, so group identity
// cannot be recycled between observing an exit and terminating descendants.
#[cfg(unix)]
fn exited(child: &mut Child) -> std::io::Result<bool> {
    use rustix::process::{waitid, Pid, WaitId, WaitIdOptions};
    let pid = Pid::from_raw(i32::try_from(child.id()).map_err(std::io::Error::other)?)
        .ok_or_else(|| std::io::Error::other("Invalid child PID"))?;
    waitid(
        WaitId::Pid(pid),
        WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
    )
    .map(|status| status.is_some())
    .map_err(Into::into)
}

#[cfg(not(unix))]
fn exited(child: &mut Child) -> std::io::Result<bool> {
    // Windows keeps the process identity reserved by its open process handle.
    #[cfg(windows)]
    {
        child.inner_mut().try_wait().map(|status| status.is_some())
    }
    #[cfg(not(windows))]
    {
        child.try_wait().map(|status| status.is_some())
    }
}

const POLL: Duration = Duration::from_millis(25);
const DIAGNOSTIC_BYTES: usize = 8 * 1024;

/// A fixed executable and its literal arguments. No shell is invoked.
#[derive(Clone, Debug)]
pub struct CommandSpec {
    /// Executable name (PATH lookup) or path.
    pub program: String,
    /// Arguments supplied by the server operator, not the MCP caller.
    pub args: Vec<String>,
}

impl CommandSpec {
    /// Parses a nonempty JSON argv array, e.g. `["cargo","run","-p","my_game"]`.
    pub fn from_json(value: &str) -> Result<Self, String> {
        let mut argv: Vec<String> = serde_json::from_str(value)
            .map_err(|e| format!("Command must be a JSON array of strings: {e}"))?;
        if argv.is_empty() || argv[0].is_empty() || argv.iter().any(|arg| arg.contains('\0')) {
            return Err("Command needs a nonempty executable and no NUL bytes".to_owned());
        }
        let program = argv.remove(0);
        Ok(Self {
            program,
            args: argv,
        })
    }

    fn command(&self, directory: &Option<PathBuf>) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).stdin(Stdio::null());
        if let Some(directory) = directory {
            command.current_dir(directory);
        }
        // Keep cargo and the binary it launches in one independently stoppable tree.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command
    }
}

/// Trusted server configuration. All durations must be nonzero and at most one hour.
#[derive(Clone, Debug)]
pub struct ProcessConfig {
    /// Command launched by `launch_game` and `restart_game`.
    pub game: CommandSpec,
    /// Optional command used by `rebuild_game` and rebuilding restarts.
    pub build: Option<CommandSpec>,
    /// Working directory shared by game and build. `None` inherits the server's cwd.
    pub directory: Option<PathBuf>,
    /// Total launch/readiness budget, including HTTP requests.
    pub ready_timeout: Duration,
    /// Grace period before forceful termination.
    pub stop_timeout: Duration,
    /// Maximum time allowed for a build.
    pub build_timeout: Duration,
}

impl ProcessConfig {
    /// Creates configuration with 30 s readiness, 3 s stop, and 300 s build budgets.
    pub fn new(game: CommandSpec) -> Self {
        Self {
            game,
            build: None,
            directory: None,
            ready_timeout: Duration::from_secs(30),
            stop_timeout: Duration::from_secs(3),
            build_timeout: Duration::from_secs(300),
        }
    }
}

/// Last observed exit, including Unix signal termination in `description`.
#[derive(Clone, Debug, Serialize)]
pub struct ProcessExit {
    /// Numeric exit code, or `None` when terminated by a signal.
    pub code: Option<i32>,
    /// Whether the command exited successfully.
    pub success: bool,
    /// Platform-specific human-readable exit status.
    pub description: String,
}

impl From<ExitStatus> for ProcessExit {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.code(),
            success: status.success(),
            description: status.to_string(),
        }
    }
}

/// Process state, independent of BRP reachability. No PID is claimed in attach mode.
#[derive(Clone, Debug, Serialize)]
pub struct ProcessStatus {
    /// Whether lifecycle tools have a configured command.
    pub configured: bool,
    /// Whether a launched process is still held by this manager.
    pub owned: bool,
    /// `attached`, `stopped`, `running`, or `exited`.
    pub state: &'static str,
    /// Live command PID (possibly cargo, rather than the game executable).
    pub pid: Option<u32>,
    /// Most recently observed game exit; cleared when a new game is launched.
    pub exit: Option<ProcessExit>,
}

/// Cooperative, thread-safe shutdown request for a process manager.
///
/// This is one-way: after cancellation no new game or build is started. The
/// owner must still drop/stop the manager to clean up a running game. Pending
/// readiness/build operations notice it between polls and clean up their trees.
#[derive(Clone, Default)]
pub struct ProcessCancellation(Arc<AtomicBool>);

impl ProcessCancellation {
    /// Requests shutdown without blocking or taking the manager's mutable borrow.
    pub fn request(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether shutdown was requested.
    pub fn is_requested(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn check(&self) -> Result<(), String> {
        if self.is_requested() {
            Err("Process operation cancelled for server shutdown".to_owned())
        } else {
            Ok(())
        }
    }
}

/// Owns at most one game tree. Lifecycle calls are synchronous and serialized.
///
/// Rebuild always stops the game first, so a failed build leaves it stopped;
/// retry `rebuild_game` or `restart_game { rebuild: true }` after fixing the code.
/// Dropping the manager stops only its own game, never an attached BRP server.
pub struct ProcessManager {
    config: Option<ProcessConfig>,
    child: Option<Child>,
    exit: Option<ProcessExit>,
    cancellation: ProcessCancellation,
}

impl ProcessManager {
    /// Creates an attach-only manager with no process ownership.
    pub fn attached() -> Self {
        Self {
            config: None,
            child: None,
            exit: None,
            cancellation: ProcessCancellation::default(),
        }
    }

    /// Validates trusted configuration without launching a command.
    pub fn new(config: ProcessConfig) -> Result<Self, String> {
        for duration in [
            config.ready_timeout,
            config.stop_timeout,
            config.build_timeout,
        ] {
            if duration.is_zero() || duration > Duration::from_secs(3600) {
                return Err("Process timeouts must be in (0, 3600] seconds".to_owned());
            }
        }
        Ok(Self {
            config: Some(config),
            child: None,
            exit: None,
            cancellation: ProcessCancellation::default(),
        })
    }

    /// Returns a cloneable shutdown handle for signal handlers or other threads.
    pub fn cancellation(&self) -> ProcessCancellation {
        self.cancellation.clone()
    }

    /// Polls for a crash/exit and returns state even when BRP is unavailable.
    pub fn status(&mut self) -> Result<ProcessStatus, String> {
        if let Some(child) = self.child.as_mut()
            && exited(child).map_err(|e| format!("Polling game: {e}"))?
        {
            // A cargo parent can exit while descendants still hold the BRP port.
            terminate_tree(child, true)?;
            let status = wait_child(child).map_err(|e| format!("Reaping game: {e}"))?;
            self.exit = Some(status.into());
            self.child = None;
        }
        Ok(ProcessStatus {
            configured: self.config.is_some(),
            owned: self.child.is_some(),
            state: if self.child.is_some() {
                "running"
            } else if self.exit.is_some() {
                "exited"
            } else if self.config.is_some() {
                "stopped"
            } else {
                "attached"
            },
            pid: self.child.as_ref().map(child_id),
            exit: self.exit.clone(),
        })
    }

    /// Returns a process-specific diagnostic before attempting BRP after a crash.
    pub fn check_game(&mut self) -> Result<(), String> {
        let status = self.status()?;
        if status.configured && !status.owned {
            return Err(match status.exit {
                Some(exit) => format!(
                    "Owned game exited: {}. Use launch_game or restart_game.",
                    exit.description
                ),
                None => "Configured game is stopped. Use launch_game.".to_owned(),
            });
        }
        Ok(())
    }

    fn config(&self) -> Result<&ProcessConfig, String> {
        self.config.as_ref().ok_or_else(|| "Attach-only mode: configure --game-cmd to enable lifecycle tools; an attached game is never stopped".to_owned())
    }

    /// Starts the fixed command and waits for a valid `rpc.discover` response.
    /// Refuses an already reachable endpoint to avoid claiming an attached game.
    /// Failed readiness cleans up the newly launched tree.
    pub fn launch(&mut self, client: &Client) -> Result<ProcessStatus, String> {
        self.cancellation.check()?;
        self.config()?;
        if self.status()?.owned {
            return Err("Game already running; use restart_game".to_owned());
        }
        let config = self.config()?;
        let deadline = Instant::now() + config.ready_timeout;
        if client
            .call_with_deadline(
                "rpc.discover",
                None,
                (Instant::now() + Duration::from_millis(250)).min(deadline),
            )
            .is_ok()
        {
            return Err(
                "BRP endpoint already reachable; refusing to launch over an attached game"
                    .to_owned(),
            );
        }
        self.cancellation.check()?;
        let child = spawn(
            config
                .game
                .command(&config.directory)
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .map_err(|e| format!("Launching configured game: {e}"))?;
        self.child = Some(child);
        self.exit = None;
        let result = self.wait_ready(client, deadline);
        if let Err(error) = result {
            self.stop()
                .map_err(|cleanup| format!("{error}; cleanup failed: {cleanup}"))?;
            return Err(error);
        }
        self.status()
    }

    fn wait_ready(&mut self, client: &Client, deadline: Instant) -> Result<(), String> {
        let mut last = "no BRP response".to_owned();
        while Instant::now() < deadline {
            self.cancellation.check()?;
            self.check_game()?;
            match client.call_with_deadline(
                "rpc.discover",
                None,
                (Instant::now() + Duration::from_millis(250)).min(deadline),
            ) {
                Ok(_) => {
                    self.check_game()?;
                    return Ok(());
                }
                Err(error) => last = error,
            }
            thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
        }
        self.check_game()?;
        Err(format!("Game readiness timed out: {last}"))
    }

    /// Gracefully stops the owned tree, then forces termination after the budget.
    /// Idempotent for a stopped configured game; an attached game is never killed.
    pub fn stop(&mut self) -> Result<ProcessStatus, String> {
        let timeout = self.config()?.stop_timeout;
        if let Some(child) = self.child.as_mut() {
            let status = stop_child(child, timeout)?;
            self.exit = Some(status.into());
            self.child = None;
        }
        self.status()
    }

    /// Stops, runs the configured build with bounded diagnostic capture, and stays
    /// stopped. Readers drain both pipes concurrently even after their cap is hit.
    pub fn rebuild(&mut self) -> Result<(), String> {
        self.cancellation.check()?;
        let config = self.config()?;
        let build = config
            .build
            .as_ref()
            .ok_or("No --build-cmd configured; game was not stopped")?
            .clone();
        self.stop()?;
        self.cancellation.check()?;
        let config = self.config()?;
        let mut child = spawn(
            build
                .command(&config.directory)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .map_err(|e| format!("Starting configured build (game is stopped): {e}"))?;
        let stdout = capture(take_stdout(&mut child).ok_or("Missing build stdout")?);
        let stderr = capture(take_stderr(&mut child).ok_or("Missing build stderr")?);
        let deadline = Instant::now() + config.build_timeout;
        let result = loop {
            if let Err(error) = self.cancellation.check() {
                break Err(error);
            }
            match exited(&mut child) {
                Ok(true) => break Ok(()),
                Ok(false) if Instant::now() < deadline => thread::sleep(POLL),
                Ok(false) => break Err("Build timed out".to_owned()),
                Err(error) => break Err(format!("Polling build: {error}")),
            }
        };
        // Also release pipes inherited by descendants after the build parent exits.
        let cleanup = terminate_tree(&mut child, true);
        let status = wait_child(&mut child).map_err(|e| format!("Reaping build: {e}"));
        let out = stdout
            .join()
            .map_err(|_| "Build stdout reader panicked")??;
        let err = stderr
            .join()
            .map_err(|_| "Build stderr reader panicked")??;
        cleanup?;
        let status = status?;
        match result {
            Ok(()) if status.success() => Ok(()),
            result => Err(format!(
                "{}; game is stopped.\nstdout:\n{out}\nstderr:\n{err}",
                match result {
                    Ok(()) => format!("Build failed: {status}"),
                    Err(error) => error,
                }
            )),
        }
    }

    /// Stops, optionally rebuilds, then launches and waits for BRP readiness.
    /// A missing build configuration is rejected before stopping the game.
    pub fn restart(&mut self, client: &Client, rebuild: bool) -> Result<ProcessStatus, String> {
        if rebuild {
            self.rebuild()?;
        } else {
            self.stop()?;
        }
        self.launch(client)
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.stop();
        }
    }
}

fn capture(mut reader: impl Read + Send + 'static) -> thread::JoinHandle<Result<String, String>> {
    thread::spawn(move || {
        let mut prefix = Vec::new();
        let mut tail = VecDeque::new();
        let mut total = 0_u64;
        let mut buffer = [0; 4096];
        loop {
            let count = reader
                .read(&mut buffer)
                .map_err(|e| format!("Reading build diagnostics: {e}"))?;
            if count == 0 {
                break;
            }
            total = total.saturating_add(count as u64);
            let retain = count.min(DIAGNOSTIC_BYTES / 2 - prefix.len());
            prefix.extend_from_slice(&buffer[..retain]);
            for byte in &buffer[retain..count] {
                if tail.len() == DIAGNOSTIC_BYTES / 2 {
                    tail.pop_front();
                }
                tail.push_back(*byte);
            }
        }
        let omitted = total.saturating_sub((prefix.len() + tail.len()) as u64);
        let mut text = String::from_utf8_lossy(&prefix).into_owned();
        if omitted > 0 {
            text.push_str(&format!("\n[truncated: {omitted} middle bytes omitted]\n"));
        }
        text.push_str(&String::from_utf8_lossy(tail.make_contiguous()));
        Ok(text)
    })
}

fn stop_child(child: &mut Child, timeout: Duration) -> Result<ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    // A failed graceful helper must never skip forceful cleanup.
    let _ = terminate_tree(child, false);
    // Reserve the leader identity throughout the grace period, including after
    // its exit. Its descendants may still be flushing/saving; leader exit is
    // NOT evidence that the tree finished. We deliberately use the full grace
    // budget rather than guessing from a zombie-containing process group.
    while Instant::now() < deadline {
        thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
    terminate_tree(child, true)?;
    wait_child(child).map_err(|e| format!("Reaping game: {e}"))
}

#[cfg(unix)]
fn terminate_tree(child: &mut Child, force: bool) -> Result<(), String> {
    use rustix::process::{kill_process_group, Pid, Signal};
    let pid = i32::try_from(child.id())
        .ok()
        .and_then(Pid::from_raw)
        .ok_or("Invalid child PID")?;
    match kill_process_group(pid, if force { Signal::KILL } else { Signal::TERM }) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        // Darwin returns EPERM for a group containing only unreaped zombies.
        // Keep the leader reserved until here, then let the caller reap it.
        #[cfg(target_os = "macos")]
        Err(rustix::io::Errno::PERM) if exited(child).unwrap_or(false) => Ok(()),
        Err(error) => Err(format!("Stopping process group: {error}")),
    }
}

#[cfg(windows)]
fn terminate_tree(child: &mut Child, force: bool) -> Result<(), String> {
    if force {
        return child
            .start_kill()
            .map_err(|e| format!("Terminating game Job Object: {e}"));
    }
    // Console applications may not support a graceful taskkill request. Bound
    // the helper too; job termination does not depend on taskkill succeeding.
    let mut helper = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Running taskkill: {e}"))?;
    let deadline = Instant::now() + Duration::from_millis(250);
    while helper.try_wait().map_err(|e| e.to_string())?.is_none() {
        if Instant::now() >= deadline {
            let _ = helper.kill();
            let _ = helper.wait();
            break;
        }
        thread::sleep(POLL);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_configuration_is_literal_json_argv() {
        for invalid in [
            "[]",
            "[\"\"]",
            "[1]",
            "null",
            "cargo run",
            "[\"cargo\\u0000\"]",
        ] {
            assert!(
                CommandSpec::from_json(invalid).is_err(),
                "accepted {invalid}"
            );
        }
        let spec = CommandSpec::from_json(
            r#"["/path with spaces/game", "", "$(echo not-a-shell)", "a;b", "--flag"]"#,
        )
        .unwrap();
        assert_eq!(spec.program, "/path with spaces/game");
        assert_eq!(spec.args, ["", "$(echo not-a-shell)", "a;b", "--flag"]);
    }

    #[test]
    fn lifecycle_timeouts_are_bounded() {
        let config = ProcessConfig::new(CommandSpec {
            program: "game".into(),
            args: Vec::new(),
        });
        for timeout in [Duration::ZERO, Duration::from_secs(3601)] {
            let mut invalid = config.clone();
            invalid.ready_timeout = timeout;
            assert!(ProcessManager::new(invalid).is_err());
            let mut invalid = config.clone();
            invalid.stop_timeout = timeout;
            assert!(ProcessManager::new(invalid).is_err());
            let mut invalid = config.clone();
            invalid.build_timeout = timeout;
            assert!(ProcessManager::new(invalid).is_err());
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn terminate_tree(child: &mut Child, _force: bool) -> Result<(), String> {
    child.kill().map_err(|e| format!("Killing child: {e}"))
}
