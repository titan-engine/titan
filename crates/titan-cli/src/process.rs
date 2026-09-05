//! Bounded retained output and wall-clock execution for Cargo workflows.
use std::{
    io::{self, Read},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
const OUTPUT_LIMIT: usize = 1024 * 1024;

pub struct Output {
    pub success: bool,
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}
#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
}
fn drain(mut reader: impl Read + Send + 'static) -> (Arc<Mutex<Capture>>, thread::JoinHandle<()>) {
    let capture = Arc::new(Mutex::new(Capture::default()));
    let copy = capture.clone();
    let handle = thread::spawn(move || {
        let mut buffer = [0; 8192];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            let mut capture = copy.lock().unwrap();
            let keep = count.min(OUTPUT_LIMIT - capture.bytes.len());
            capture.bytes.extend_from_slice(&buffer[..keep]);
            capture.truncated |= keep < count;
        }
    });
    (capture, handle)
}
fn text(capture: &Mutex<Capture>) -> String {
    let capture = capture.lock().unwrap();
    let mut text = String::from_utf8_lossy(&capture.bytes).into_owned();
    if capture.truncated {
        text.push_str("\n[Titan: output truncated at 1 MiB]\n");
    }
    text
}
fn terminate(child: &mut Child) {
    #[cfg(unix)]
    // The spawned child leads a fresh process group. Kill Cargo and ordinary
    // test/example descendants together; no user shell process shares this ID.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}
pub fn run(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let started = Instant::now();
    let mut child = command.spawn()?;
    let (stdout, out_thread) = drain(child.stdout.take().unwrap());
    let (stderr, err_thread) = drain(child.stderr.take().unwrap());
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
            Ok(None) => {}
            Err(error) => {
                terminate(&mut child);
                let _ = child.wait();
                return Err(error);
            }
        }
        if started.elapsed() >= timeout {
            terminate(&mut child);
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(5));
    };
    // Clean up background descendants even when Cargo itself has exited.
    terminate(&mut child);
    // A descendant could deliberately escape the process group and retain a
    // pipe. Never wait indefinitely for reader EOF in that case.
    let drain_started = Instant::now();
    while !(out_thread.is_finished() && err_thread.is_finished())
        && drain_started.elapsed() < Duration::from_millis(100)
    {
        thread::sleep(Duration::from_millis(1));
    }
    let mut stderr = text(&stderr);
    if timed_out {
        stderr.push_str("\nTitan process timeout: wall-clock budget exceeded.\n");
    }
    Ok(Output {
        success: status.success() && !timed_out,
        timed_out,
        exit_code: status.code(),
        stdout: text(&stdout),
        stderr,
    })
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn captures_exit_failure_and_both_streams() {
        let out = run(
            Command::new("sh").args(["-c", "printf hello; printf error >&2; exit 7"]),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(!out.success);
        assert_eq!(out.exit_code, Some(7));
        assert_eq!(out.stdout, "hello");
        assert_eq!(out.stderr, "error");
    }
    #[test]
    fn kills_long_running_process_group() {
        let start = Instant::now();
        let out = run(
            Command::new("sh").args(["-c", "sleep 30 & wait"]),
            Duration::from_millis(30),
        )
        .unwrap();
        assert!(!out.success);
        assert!(out.stderr.contains("wall-clock budget exceeded"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn noisy_child_cannot_grow_retained_output_without_bound() {
        // Produce a fixed amount of output before exiting. A short timeout on
        // an infinite producer makes truncation depend on runner throughput.
        let out = run(
            Command::new("dd").args(["if=/dev/zero", "bs=1048576", "count=2"]),
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(out.success, "{}", out.stderr);
        assert!(!out.timed_out);
        let marker = "\n[Titan: output truncated at 1 MiB]\n";
        assert_eq!(out.stdout.len(), OUTPUT_LIMIT + marker.len());
        assert!(
            out.stdout.as_bytes()[..OUTPUT_LIMIT]
                .iter()
                .all(|&b| b == 0)
        );
        assert!(out.stdout.ends_with(marker));
    }
}
