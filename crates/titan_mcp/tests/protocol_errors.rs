//! BRP errors must remain bounded, actionable, and safe for newline-delimited MCP.

extern crate alloc;

use alloc::sync::Arc;
use serde_json::{json, Value};
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_TEXT_BYTES: usize = 24 * 1024;
const WAIT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(5);
const METHOD: &str = "test.invalid_component";
const ERROR_CODE: i32 = -32602;

// One owned worker handles both requests. Nonblocking sockets and a shared stop
// flag bound accept/read/write waits even if an assertion unwinds the test.
struct ErrorServer {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<usize>>>,
}

impl ErrorServer {
    fn start(messages: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let mut served = 0;
            for message in messages {
                let deadline = Instant::now() + WAIT;
                let mut stream = loop {
                    check_wait(&worker_stop, deadline)?;
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(POLL);
                        }
                        Err(error) => return Err(error),
                    }
                };
                stream.set_nonblocking(true)?;
                let request = read_request(&mut stream, &worker_stop, deadline)?;
                assert_eq!(request["method"], METHOD);
                assert_eq!(request["params"], json!({"type": "MissingComponent"}));
                assert!(request["id"].is_number());
                let body = json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "error": {"code": ERROR_CODE, "message": message}
                })
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let mut remaining = response.as_bytes();
                while !remaining.is_empty() {
                    check_wait(&worker_stop, deadline)?;
                    match stream.write(remaining) {
                        Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                        Ok(written) => remaining = &remaining[written..],
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(POLL);
                        }
                        Err(error) => return Err(error),
                    }
                }
                served += 1;
            }
            Ok(served)
        });
        Self {
            url,
            stop,
            worker: Some(worker),
        }
    }

    fn finish(&mut self) {
        assert_eq!(self.worker.take().unwrap().join().unwrap().unwrap(), 2);
    }
}

impl Drop for ErrorServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            // Do not double-panic during cleanup; finish reports worker failures.
            let _ = worker.join();
        }
    }
}

fn check_wait(stop: &AtomicBool, deadline: Instant) -> io::Result<()> {
    if stop.load(Ordering::Relaxed) || Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "BRP fixture stopped or exceeded its I/O deadline",
        ))
    } else {
        Ok(())
    }
}

fn read_request(stream: &mut TcpStream, stop: &AtomicBool, deadline: Instant) -> io::Result<Value> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        check_wait(stop, deadline)?;
        match stream.read(&mut buffer) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(length) => bytes.extend_from_slice(&buffer[..length]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(POLL);
                continue;
            }
            Err(error) => return Err(error),
        }
        if bytes.len() > 64 * 1024 {
            return Err(io::Error::other("unexpectedly large HTTP request"));
        }
        if let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..header_end])
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            assert!(headers.starts_with("POST / HTTP/1.1\r\n"));
            let length: usize = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .expect("HTTP request needs Content-Length")
                .1
                .trim()
                .parse()
                .unwrap();
            let body_start = header_end + 4;
            if length > 64 * 1024 {
                return Err(io::Error::other("unexpectedly large HTTP body"));
            }
            if bytes.len() >= body_start + length {
                return serde_json::from_slice(&bytes[body_start..body_start + length])
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
            }
        }
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn exchange(url: &str, requests: &[Value]) -> (Vec<Value>, usize) {
    // File-backed stdio avoids detached pipe readers or a child blocking on a
    // full stdout pipe. All descriptors and the child are lifetime-owned.
    let mut input = tempfile::tempfile().unwrap();
    for request in requests {
        writeln!(input, "{request}").unwrap();
    }
    input.rewind().unwrap();
    let mut stdout = tempfile::tempfile().unwrap();
    let mut stderr = tempfile::tempfile().unwrap();
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_titan_mcp"))
            .args(["--url", url])
            .stdin(Stdio::from(input))
            .stdout(Stdio::from(stdout.try_clone().unwrap()))
            .stderr(Stdio::from(stderr.try_clone().unwrap()))
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + WAIT;
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "MCP child exceeded its deadline");
        thread::sleep(POLL);
    };
    stderr.rewind().unwrap();
    let mut diagnostics = String::new();
    stderr
        .take(64 * 1024)
        .read_to_string(&mut diagnostics)
        .unwrap();
    assert!(status.success(), "MCP failed: {diagnostics}");
    let output_bytes = usize::try_from(stdout.seek(SeekFrom::End(0)).unwrap()).unwrap();
    // Allow the old implementation's large error to be parsed, so the regression
    // fails specifically on the text bound rather than pipe capacity or a hang.
    assert!(output_bytes < 1024 * 1024, "runaway MCP output");
    stdout.rewind().unwrap();
    let mut output = String::new();
    stdout.read_to_string(&mut output).unwrap();
    assert!(
        output.ends_with('\n'),
        "last response must end with a newline"
    );
    let responses = output
        .split_terminator('\n')
        .map(|line| serde_json::from_str(line).expect("each stdout line must be valid JSON"))
        .collect();
    (responses, output_bytes)
}

fn error_text(response: &Value) -> &str {
    assert_eq!(response["jsonrpc"], "2.0");
    assert!(
        response.get("error").is_none(),
        "expected a tool error, not an RPC error"
    );
    assert_eq!(response["result"]["isError"], true);
    let content = response["result"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
    content[0]["text"].as_str().unwrap()
}

fn actionable_error(message: &str) -> String {
    format!(
        "BRP {METHOD} error {ERROR_CODE}: {message}. Check the method parameters and use find_types to check reflected, registered types."
    )
}

#[test]
fn oversized_brp_errors_are_bounded_and_small_errors_stay_actionable() {
    // Include multibyte UTF-8 and characters that grow when JSON-escaped. The
    // returned text bound must apply to serialized metadata, not just its prefix.
    let large = "MissingComponent 🦀: \"bad type\"\n\t\0\\\u{1f} ".repeat(8192);
    assert!(large.len() >= 256 * 1024);
    let small = "No registered type named MissingComponent; try game::Player 🦀";
    let full_error = actionable_error(&large);
    let mut server = ErrorServer::start(vec![large, small.to_owned()]);
    let call = |id| {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{
            "name":"brp_call","arguments":{"method":METHOD,"params":{"type":"MissingComponent"}}
        }})
    };
    let (responses, output_bytes) = exchange(
        &server.url,
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-11-25","capabilities":{},
                "clientInfo":{"name":"protocol-errors-test","version":"1"}
            }}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            call(2),
            json!({"jsonrpc":"2.0","id":3,"method":"ping"}),
            call(4),
            json!({"jsonrpc":"2.0","id":5,"method":"ping"}),
        ],
    );
    server.finish();
    assert_eq!(responses.len(), 5);
    for (index, response) in responses.iter().enumerate() {
        assert_eq!(response["id"], index + 1);
        assert_eq!(response["jsonrpc"], "2.0");
    }
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "titan_mcp");
    let text = error_text(&responses[1]);
    assert!(
        text.len() <= MAX_TEXT_BYTES,
        "BRP error text exceeds 24 KiB: {} bytes",
        text.len()
    );
    let metadata: Value =
        serde_json::from_str(text).expect("truncation metadata must be valid JSON");
    assert_eq!(metadata["truncated"], true);
    let prefix = metadata["error_prefix"].as_str().unwrap();
    assert!(prefix.starts_with(&format!("BRP {METHOD} error {ERROR_CODE}: ")));
    assert!(
        full_error.starts_with(prefix),
        "prefix must preserve original UTF-8 text"
    );
    assert!(prefix.contains("🦀"));
    assert!(prefix.contains("\"bad type\"\n\t\0\\\u{1f}"));
    let omitted = metadata["omitted_bytes"].as_u64().unwrap();
    assert_eq!(
        omitted,
        u64::try_from(full_error.len() - prefix.len()).unwrap()
    );
    assert!(
        omitted > 256 * 1024,
        "omission count should describe the large error"
    );
    let note = metadata["note"].as_str().unwrap().to_lowercase();
    assert!(
        note.contains("param"),
        "guidance should recommend narrower parameters"
    );
    assert!(
        note.contains("type"),
        "guidance should recommend a more specific type"
    );
    assert!(
        note.contains("log"),
        "guidance should point to full game logs"
    );
    assert_eq!(error_text(&responses[3]), actionable_error(small));
    for index in [2, 4] {
        assert!(responses[index].get("error").is_none());
        assert_eq!(responses[index]["result"], json!({}));
    }
    assert!(
        output_bytes < 64 * 1024,
        "overall protocol output must stay bounded"
    );
}
