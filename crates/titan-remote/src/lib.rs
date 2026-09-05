//! Native loopback transport. Only `RequestQueue::drain` calls runtime code.
use serde::{Deserialize, Serialize};
use std::{
    fmt, fs,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use titan_protocol::{RequestEnvelope, ResponseEnvelope, RunMode, SCHEMA_VERSION};
const MAX_BODY: usize = 4 * 1024 * 1024;
const MAX_HEADER: usize = 8192;
const MAX_WORKERS: usize = 16;

#[derive(Debug)]
pub enum RemoteError {
    Io(io::Error),
    Json(serde_json::Error),
    Invalid(String),
    Unauthorized,
    Busy,
    Timeout,
    NotFound,
    AmbiguousTarget,
}
impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
            Self::Invalid(e) => write!(f, "{e}"),
            Self::Unauthorized => write!(f, "authentication failed"),
            Self::Busy => write!(f, "runtime queue is busy"),
            Self::Timeout => write!(f, "runtime request timed out"),
            Self::NotFound => write!(f, "no matching runtime instance"),
            Self::AmbiguousTarget => write!(
                f,
                "multiple runtime instances match; choose an instance explicitly"
            ),
        }
    }
}
impl std::error::Error for RemoteError {}
impl From<io::Error> for RemoteError {
    fn from(e: io::Error) -> Self {
        if matches!(
            e.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ) {
            Self::Timeout
        } else {
            Self::Io(e)
        }
    }
}
impl From<serde_json::Error> for RemoteError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Registration {
    pub instance_id: String,
    pub project: PathBuf,
    pub pid: u32,
    pub endpoint: String,
    pub schema_version: u32,
    pub run_mode: RunMode,
    pub token: String,
}
impl fmt::Debug for Registration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registration")
            .field("instance_id", &self.instance_id)
            .field("project", &self.project)
            .field("pid", &self.pid)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}
pub fn registry_dir(project: &Path) -> PathBuf {
    project.join("target/titan/instances")
}
pub fn default_registry_dir() -> PathBuf {
    registry_dir(Path::new("."))
}
pub struct ServerConfig {
    pub project: PathBuf,
    pub registry_dir: PathBuf,
    pub instance_id: String,
    pub run_mode: RunMode,
    pub queue_capacity: usize,
    pub request_timeout: Duration,
}
impl ServerConfig {
    pub fn new(
        project: impl Into<PathBuf>,
        instance_id: impl Into<String>,
        run_mode: RunMode,
    ) -> Self {
        let project = project.into();
        Self {
            registry_dir: registry_dir(&project),
            project,
            instance_id: instance_id.into(),
            run_mode,
            queue_capacity: 64,
            request_timeout: Duration::from_secs(5),
        }
    }
}
struct Pending {
    request: RequestEnvelope,
    deadline: Instant,
    response: SyncSender<ResponseEnvelope>,
}
/// Owned reply endpoint, detached from the runtime safe-point borrow.
/// Dropping it cancels delivery; a transport timeout never rolls back runtime work.
pub struct ReplyHandle {
    response: SyncSender<ResponseEnvelope>,
    deadline: Instant,
    stop: Arc<AtomicBool>,
}
impl ReplyHandle {
    pub fn send(self, response: ResponseEnvelope) {
        if !self.stop.load(Ordering::Acquire) && Instant::now() < self.deadline {
            let _ = self.response.try_send(response);
        }
    }

    /// Drive owned completion independently of the simulation/window loop.
    /// The transport's bounded deadline also bounds this worker. The callback
    /// must be nonblocking and must not capture an App or runtime lock.
    pub fn complete_when(
        self,
        started: Instant,
        mut poll: impl FnMut(Duration) -> Option<ResponseEnvelope> + Send + 'static,
    ) {
        thread::spawn(move || {
            while !self.stop.load(Ordering::Acquire) && Instant::now() < self.deadline {
                if let Some(response) = poll(started.elapsed()) {
                    self.send(response);
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
        });
    }
}

pub struct RequestQueue {
    receiver: Receiver<Pending>,
    stop: Arc<AtomicBool>,
    capacity: usize,
}
impl RequestQueue {
    /// Drain at a runtime safe point. Expired requests never begin execution.
    /// Once a handler starts, a client timeout does not roll back its effects.
    pub fn drain(&self, mut handler: impl FnMut(&RequestEnvelope) -> ResponseEnvelope) -> usize {
        self.drain_with_reply(|request, reply| reply.send(handler(request)))
    }

    /// Accept requests at a safe point and move deferred replies out of that
    /// borrow. Authentication and expiration are checked before dispatch.
    pub fn drain_with_reply(
        &self,
        mut handler: impl FnMut(&RequestEnvelope, ReplyHandle),
    ) -> usize {
        let mut count = 0;
        for _ in 0..self.capacity {
            let Ok(pending) = self.receiver.try_recv() else {
                break;
            };
            if !self.stop.load(Ordering::Acquire) && Instant::now() < pending.deadline {
                handler(
                    &pending.request,
                    ReplyHandle {
                        response: pending.response,
                        deadline: pending.deadline,
                        stop: self.stop.clone(),
                    },
                );
                count += 1;
            }
        }
        count
    }
}
pub struct Server {
    registration: Registration,
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Server {
    pub fn start(config: ServerConfig) -> Result<(Self, RequestQueue), RemoteError> {
        if config.queue_capacity == 0
            || config.request_timeout.is_zero()
            || config.request_timeout > Duration::from_secs(30)
        {
            return Err(RemoteError::Invalid(
                "queue capacity must be positive and timeout must be in (0, 30s]".into(),
            ));
        }
        let project = config.project.canonicalize()?;
        if config.instance_id.is_empty() {
            return Err(RemoteError::Invalid("instance id cannot be empty".into()));
        }
        private_dir(&config.registry_dir)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let token = random_token()?;
        let registration = Registration {
            instance_id: config.instance_id,
            project,
            pid: std::process::id(),
            endpoint: format!("http://{}/request", listener.local_addr()?),
            schema_version: SCHEMA_VERSION,
            run_mode: config.run_mode,
            token: token.clone(),
        };
        let path = config
            .registry_dir
            .join(format!("{}-{}.json", registration.pid, &token[..16]));
        private_write(&path, &serde_json::to_vec(&registration)?)?;
        let (sender, receiver) = mpsc::sync_channel(config.queue_capacity);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let stopping_queue = stop.clone();
        let thread = thread::spawn(move || {
            let active = Arc::new(AtomicUsize::new(0));
            let mut workers = Vec::new();
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if active.load(Ordering::Acquire) >= MAX_WORKERS {
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(50)));
                            let _ = reply(&mut stream, 503, b"busy");
                            continue;
                        }
                        active.fetch_add(1, Ordering::AcqRel);
                        let active = active.clone();
                        let sender = sender.clone();
                        let token = token.clone();
                        let stopping = stopping.clone();
                        workers.push(thread::spawn(move || {
                            let _ = serve(
                                &mut stream,
                                &token,
                                &sender,
                                config.request_timeout,
                                &stopping,
                            );
                            active.fetch_sub(1, Ordering::AcqRel);
                        }));
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
                let mut i = 0;
                while i < workers.len() {
                    if workers[i].is_finished() {
                        let _ = workers.swap_remove(i).join();
                    } else {
                        i += 1;
                    }
                }
            }
            for worker in workers {
                let _ = worker.join();
            }
        });
        Ok((
            Self {
                registration,
                path,
                stop,
                thread: Some(thread),
            },
            RequestQueue {
                receiver,
                stop: stopping_queue,
                capacity: config.queue_capacity,
            },
        ))
    }
    pub fn registration(&self) -> &Registration {
        &self.registration
    }
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = fs::remove_file(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn random_token() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
#[cfg(unix)]
fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "registry must be an owner-only directory",
        ));
    }
    Ok(())
}
#[cfg(unix)]
fn private_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}
fn endpoint(registration: &Registration) -> Result<SocketAddr, RemoteError> {
    let address = registration
        .endpoint
        .strip_prefix("http://")
        .and_then(|s| s.strip_suffix("/request"))
        .ok_or_else(|| RemoteError::Invalid("invalid runtime endpoint".into()))?
        .parse::<SocketAddr>()
        .map_err(|_| RemoteError::Invalid("invalid runtime address".into()))?;
    if !address.ip().is_loopback() {
        return Err(RemoteError::Invalid(
            "runtime endpoint must be loopback".into(),
        ));
    }
    Ok(address)
}
/// Ignore stale or malformed files. Discovery never deletes another process's files.
pub fn discover(directory: &Path, project: &Path) -> Result<Vec<Registration>, RemoteError> {
    let project = project.canonicalize()?;
    let entries = match fs::read_dir(directory) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > 16384 {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
                continue;
            }
        }
        let Ok(bytes) = fs::read(path) else { continue };
        let Ok(reg) = serde_json::from_slice::<Registration>(&bytes) else {
            continue;
        };
        if reg.project != project || reg.pid == 0 || reg.pid > i32::MAX as u32 {
            continue;
        }
        if unsafe { libc::kill(reg.pid as i32, 0) } != 0 {
            continue;
        }
        let Ok(address) = endpoint(&reg) else {
            continue;
        };
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            found.push(reg);
        }
    }
    found.sort_by(|a, b| {
        a.instance_id
            .cmp(&b.instance_id)
            .then(a.endpoint.cmp(&b.endpoint))
    });
    Ok(found)
}
pub fn select(
    registrations: &[Registration],
    instance: Option<&str>,
) -> Result<Registration, RemoteError> {
    let mut matches = registrations
        .iter()
        .filter(|r| instance.is_none_or(|id| r.instance_id == id));
    let first = matches.next().ok_or(RemoteError::NotFound)?;
    if matches.next().is_some() {
        return Err(RemoteError::AmbiguousTarget);
    }
    Ok(first.clone())
}
// Quantize to milliseconds before the OS timeval conversion. Fractional
// microseconds near a second boundary can produce an invalid timeval on macOS.
fn socket_timeout(remaining: Duration) -> Duration {
    Duration::from_millis(remaining.as_millis().clamp(1, u64::MAX as u128) as u64)
}
fn read_message(
    stream: &mut TcpStream,
    timeout: Duration,
) -> Result<(String, Vec<u8>), RemoteError> {
    let deadline = Instant::now() + timeout;
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= MAX_HEADER {
            return Err(RemoteError::Invalid("headers too large".into()));
        }
        stream.set_read_timeout(Some(socket_timeout(
            deadline.saturating_duration_since(Instant::now()),
        )))?;
        if Instant::now() >= deadline {
            return Err(RemoteError::Timeout);
        }
        let mut byte = [0];
        stream.read_exact(&mut byte)?;
        header.push(byte[0]);
    }
    let header = String::from_utf8(header)
        .map_err(|_| RemoteError::Invalid("invalid HTTP header".into()))?;
    let mut length = None;
    for line in header.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("transfer-encoding") {
                return Err(RemoteError::Invalid("transfer encoding unsupported".into()));
            }
            if name.eq_ignore_ascii_case("content-length") {
                if length.is_some() {
                    return Err(RemoteError::Invalid("duplicate content length".into()));
                }
                length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|_| RemoteError::Invalid("invalid content length".into()))?,
                );
            }
        }
    }
    let length = length.ok_or_else(|| RemoteError::Invalid("content length required".into()))?;
    if length > MAX_BODY {
        return Err(RemoteError::Invalid("body too large".into()));
    }
    let mut body = vec![0; length];
    let mut offset = 0;
    while offset < length {
        if Instant::now() >= deadline {
            return Err(RemoteError::Timeout);
        }
        stream.set_read_timeout(Some(socket_timeout(
            deadline.saturating_duration_since(Instant::now()),
        )))?;
        let n = stream.read(&mut body[offset..])?;
        if n == 0 {
            return Err(RemoteError::Invalid("incomplete body".into()));
        }
        offset += n;
    }
    Ok((header, body))
}
fn write_until(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
) -> Result<(), RemoteError> {
    while !bytes.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(RemoteError::Timeout);
        }
        stream.set_write_timeout(Some(socket_timeout(remaining)))?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "connection closed").into());
            }
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn reply(stream: &mut TcpStream, status: u16, body: &[u8]) -> Result<(), RemoteError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let header = format!(
        "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    write_until(stream, header.as_bytes(), deadline)?;
    write_until(stream, body, deadline)
}
fn serve(
    stream: &mut TcpStream,
    token: &str,
    sender: &SyncSender<Pending>,
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<(), RemoteError> {
    // BSD/macOS accepted sockets inherit the listener nonblocking flag.
    stream.set_nonblocking(false)?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let (header, body) = match read_message(stream, Duration::from_secs(2)) {
        Ok(v) => v,
        Err(error) => {
            reply(stream, 400, error.to_string().as_bytes())?;
            return Ok(());
        }
    };
    let mut lines = header.lines();
    if lines.next() != Some("POST /request HTTP/1.1") {
        reply(stream, 404, b"not found")?;
        return Ok(());
    }
    if header
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .any(|(k, _)| k.eq_ignore_ascii_case("origin"))
    {
        reply(stream, 403, b"browser origins forbidden")?;
        return Ok(());
    }
    let expected = format!("Bearer {token}");
    let auth: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .collect();
    if auth.len() != 1 || auth[0].1.trim() != expected {
        reply(stream, 401, b"unauthorized")?;
        return Ok(());
    }
    let request = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => {
            reply(stream, 400, b"invalid JSON")?;
            return Ok(());
        }
    };
    // Client and server share the host clock. Honor the client's total deadline
    // as well as the server limit so abandoned queued mutations cannot start.
    let deadlines: Vec<_> = header
        .lines()
        .filter_map(|l| l.split_once(':'))
        .filter(|(k, _)| k.eq_ignore_ascii_case("x-titan-deadline-unix-ms"))
        .collect();
    let timeout = match deadlines.as_slice() {
        [] => timeout,
        [(_, value)] => match value.trim().parse::<u128>() {
            Ok(deadline) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                timeout.min(Duration::from_millis(
                    deadline.saturating_sub(now).min(u64::MAX as u128) as u64,
                ))
            }
            Err(_) => {
                reply(stream, 400, b"invalid deadline")?;
                return Ok(());
            }
        },
        _ => {
            reply(stream, 400, b"duplicate deadline")?;
            return Ok(());
        }
    };
    if timeout.is_zero() {
        reply(stream, 504, b"timeout")?;
        return Ok(());
    }
    let (response, receiver) = mpsc::sync_channel(1);
    let deadline = Instant::now() + timeout;
    match sender.try_send(Pending {
        request,
        deadline,
        response,
    }) {
        Ok(()) => {}
        Err(_) => {
            reply(stream, 503, b"busy")?;
            return Ok(());
        }
    }
    loop {
        if stop.load(Ordering::Acquire) {
            reply(stream, 503, b"stopping")?;
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            reply(stream, 504, b"timeout")?;
            break;
        }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(10))) {
            Ok(response) => {
                reply(stream, 200, &serde_json::to_vec(&response)?)?;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                reply(stream, 503, b"runtime unavailable")?;
                break;
            }
        }
    }
    Ok(())
}
pub fn send(
    registration: &Registration,
    request: &RequestEnvelope,
    timeout: Duration,
) -> Result<ResponseEnvelope, RemoteError> {
    if timeout.is_zero() {
        return Err(RemoteError::Timeout);
    }
    let started = Instant::now();
    let unix_deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .saturating_add(timeout.as_millis());
    let deadline = started
        .checked_add(timeout)
        .ok_or_else(|| RemoteError::Invalid("timeout too large".into()))?;
    let mut stream = TcpStream::connect_timeout(&endpoint(registration)?, socket_timeout(timeout))?;
    let body = serde_json::to_vec(request)?;
    if body.len() > MAX_BODY {
        return Err(RemoteError::Invalid("request body too large".into()));
    }
    if registration.token.bytes().any(|b| !b.is_ascii_hexdigit()) {
        return Err(RemoteError::Invalid("invalid token".into()));
    }
    let header = format!(
        "POST /request HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nX-Titan-Deadline-Unix-Ms: {unix_deadline}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        registration.token,
        body.len()
    );
    write_until(&mut stream, header.as_bytes(), deadline)?;
    write_until(&mut stream, &body, deadline)?;
    let (header, body) = read_message(&mut stream, timeout.saturating_sub(started.elapsed()))?;
    match header.split_whitespace().nth(1) {
        Some("200") => {
            let response: ResponseEnvelope = serde_json::from_slice(&body)?;
            if response.request_id != request.request_id
                || response.instance_id != registration.instance_id
                || response.schema_version != SCHEMA_VERSION
            {
                return Err(RemoteError::Invalid(
                    "response correlation or schema mismatch".into(),
                ));
            }
            Ok(response)
        }
        Some("401") => Err(RemoteError::Unauthorized),
        Some("503") => Err(RemoteError::Busy),
        Some("504") => Err(RemoteError::Timeout),
        _ => Err(RemoteError::Invalid(format!(
            "unexpected HTTP response: {} ({})",
            header.lines().next().unwrap_or(""),
            String::from_utf8_lossy(&body)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use titan_protocol::{Request, Response};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("titan-remote-test-{}", random_token().unwrap()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn config(&self) -> ServerConfig {
            ServerConfig::new(&self.0, "test-instance", RunMode::Headless)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn request() -> RequestEnvelope {
        RequestEnvelope::new("test-request", Request::Status)
    }
    fn response(request: &RequestEnvelope) -> ResponseEnvelope {
        ResponseEnvelope::success(
            request,
            "test-instance",
            12,
            4,
            Response::Commands { commands: vec![] },
        )
    }
    fn wait_queued(queue: &RequestQueue) -> Pending {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(pending) = queue.receiver.try_recv() {
                return pending;
            }
            assert!(Instant::now() < deadline, "request never queued");
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn authenticated_round_trip_and_shutdown_registration_cleanup() {
        let fixture = Fixture::new();
        let (mut server, queue) = Server::start(fixture.config()).unwrap();
        let registration = server.registration().clone();
        let file = server.path.clone();
        let client = thread::spawn(move || send(&registration, &request(), Duration::from_secs(2)));
        let pending = wait_queued(&queue);
        pending.response.send(response(&pending.request)).unwrap();
        let actual = client.join().unwrap().unwrap();
        assert_eq!(actual.observed_frame, 12);
        assert_eq!(actual.state_revision, 4);
        server.shutdown();
        assert!(!file.exists());
        assert!(TcpStream::connect(endpoint(server.registration()).unwrap()).is_err());
    }
    #[test]
    fn authentication_fails_before_queueing() {
        let fixture = Fixture::new();
        let (server, queue) = Server::start(fixture.config()).unwrap();
        let mut registration = server.registration().clone();
        registration.token = "00".repeat(32);
        assert!(matches!(
            send(&registration, &request(), Duration::from_secs(1)),
            Err(RemoteError::Unauthorized)
        ));
        assert_eq!(queue.drain(response), 0);
    }
    #[test]
    fn browser_origin_is_rejected() {
        let fixture = Fixture::new();
        let (server, queue) = Server::start(fixture.config()).unwrap();
        let mut stream = TcpStream::connect(endpoint(server.registration()).unwrap()).unwrap();
        write!(stream,"POST /request HTTP/1.1\r\nOrigin: https://example.com\r\nAuthorization: Bearer {}\r\nContent-Length: 2\r\n\r\n{{}}",server.registration().token).unwrap();
        let (header, _) = read_message(&mut stream, Duration::from_secs(1)).unwrap();
        assert!(header.starts_with("HTTP/1.1 403"));
        assert_eq!(queue.drain(response), 0);
    }
    #[test]
    fn expired_requests_never_execute_and_queue_is_bounded() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.queue_capacity = 1;
        config.request_timeout = Duration::from_millis(150);
        let (server, queue) = Server::start(config).unwrap();
        let registration = server.registration().clone();
        let first = thread::spawn(move || send(&registration, &request(), Duration::from_secs(2)));
        // Wait until the first request occupies the queue, then restore it via the
        // runtime-independent channel test below to avoid timing-dependent fullness.
        let pending = wait_queued(&queue);
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.try_send(pending).unwrap();
        assert!(matches!(
            sender.try_send(Pending {
                request: request(),
                deadline: Instant::now() + Duration::from_secs(1),
                response: mpsc::sync_channel(1).0
            }),
            Err(mpsc::TrySendError::Full(_))
        ));
        assert!(matches!(first.join().unwrap(), Err(RemoteError::Timeout)));
        let expired = RequestQueue {
            receiver,
            stop: Arc::new(AtomicBool::new(false)),
            capacity: 1,
        };
        assert_eq!(expired.drain(|_| panic!("expired request executed")), 0);
    }
    #[test]
    fn queue_busy_is_reported_over_http() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config.queue_capacity = 1;
        config.request_timeout = Duration::from_millis(300);
        let (server, _queue) = Server::start(config).unwrap();
        // A timed-out request stays queued until the runtime drains it. Wait for
        // that response so the queue is full before issuing the second request.
        let mut stream = TcpStream::connect(endpoint(server.registration()).unwrap()).unwrap();
        let body = serde_json::to_vec(&request()).unwrap();
        write!(
            stream,
            "POST /request HTTP/1.1\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\n\r\n",
            server.registration().token,
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        let (header, _) = read_message(&mut stream, Duration::from_secs(2)).unwrap();
        assert!(header.starts_with("HTTP/1.1 504"));
        assert!(matches!(
            send(server.registration(), &request(), Duration::from_secs(2)),
            Err(RemoteError::Busy)
        ));
    }
    #[test]
    fn discovery_filters_stale_and_other_projects_and_requires_selection() {
        let fixture = Fixture::new();
        let (server, _queue) = Server::start(fixture.config()).unwrap();
        let dir = registry_dir(&fixture.0);
        let mut stale = server.registration().clone();
        stale.pid = i32::MAX as u32;
        private_write(
            &dir.join("stale.json"),
            &serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        private_write(&dir.join("broken.json"), b"{").unwrap();
        let found = discover(&dir, &fixture.0).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(select(&found, None).unwrap().instance_id, "test-instance");
        assert!(matches!(
            select(&[found[0].clone(), found[0].clone()], None),
            Err(RemoteError::AmbiguousTarget)
        ));
        assert!(matches!(
            select(&found, Some("missing")),
            Err(RemoteError::NotFound)
        ));
        let other = Fixture::new();
        assert!(discover(&dir, &other.0).unwrap().is_empty());
    }
    #[test]
    fn shutdown_cancels_queued_work() {
        let fixture = Fixture::new();
        let (mut server, queue) = Server::start(fixture.config()).unwrap();
        let registration = server.registration().clone();
        let client = thread::spawn(move || send(&registration, &request(), Duration::from_secs(2)));
        let pending = wait_queued(&queue);
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(pending).unwrap();
        let queue = RequestQueue {
            receiver,
            stop: queue.stop.clone(),
            capacity: 1,
        };
        server.shutdown();
        assert!(matches!(client.join().unwrap(), Err(RemoteError::Busy)));
        assert_eq!(queue.drain(|_| panic!("stopped work executed")), 0);
    }
    #[test]
    fn deferred_reply_completes_without_another_safe_point() {
        let fixture = Fixture::new();
        let (server, queue) = Server::start(fixture.config()).unwrap();
        let registration = server.registration().clone();
        let client = thread::spawn(move || send(&registration, &request(), Duration::from_secs(2)));
        let released = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let released = released.clone();
            if queue.drain_with_reply(|request, reply| {
                let response = response(request);
                let released = released.clone();
                reply.complete_when(Instant::now(), move |_| {
                    released.load(Ordering::Acquire).then(|| response.clone())
                });
            }) == 1
            {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert!(!client.is_finished());
        // A paused host performs no more queue drain or simulation tick.
        released.store(true, Ordering::Release);
        let result = client.join().unwrap().unwrap();
        assert_eq!(result.request_id, request().request_id);
    }

    #[test]
    fn oversized_headers_and_bodies_are_rejected() {
        let fixture = Fixture::new();
        let (server, queue) = Server::start(fixture.config()).unwrap();
        for length in [MAX_BODY + 1, usize::MAX] {
            let mut stream = TcpStream::connect(endpoint(server.registration()).unwrap()).unwrap();
            write!(
                stream,
                "POST /request HTTP/1.1\r\nContent-Length: {length}\r\n\r\n"
            )
            .unwrap();
            let (header, _) = read_message(&mut stream, Duration::from_secs(1)).unwrap();
            assert!(header.starts_with("HTTP/1.1 400"));
        }
        assert_eq!(queue.drain(response), 0);
    }
    #[test]
    fn fragmented_http_headers_wait_for_remaining_bytes() {
        let fixture = Fixture::new();
        let (server, queue) = Server::start(fixture.config()).unwrap();
        let registration = server.registration().clone();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(endpoint(&registration).unwrap()).unwrap();
            stream.write_all(b"POST /request HTTP/1.1\r\n").unwrap();
            thread::sleep(Duration::from_millis(20));
            let body = serde_json::to_vec(&request()).unwrap();
            write!(
                stream,
                "Authorization: Bearer {}\r\nContent-Length: {}\r\n\r\n",
                registration.token,
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
            read_message(&mut stream, Duration::from_secs(2)).unwrap().0
        });
        let pending = wait_queued(&queue);
        pending.response.send(response(&pending.request)).unwrap();
        assert!(client.join().unwrap().starts_with("HTTP/1.1 200"));
    }

    #[test]
    fn shorter_client_deadline_cancels_queued_mutation() {
        let fixture = Fixture::new();
        let (server, queue) = Server::start(fixture.config()).unwrap();
        let request = RequestEnvelope::new("short", Request::Step { frames: 1 });
        assert!(matches!(
            send(server.registration(), &request, Duration::from_millis(40)),
            Err(RemoteError::Timeout)
        ));
        // Allow the header's millisecond clock rounding to settle.
        thread::sleep(Duration::from_millis(5));
        assert_eq!(
            queue.drain(|_| panic!("client-expired mutation executed")),
            0
        );
    }

    #[test]
    fn insecure_registry_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        let config = fixture.config();
        fs::create_dir_all(&config.registry_dir).unwrap();
        fs::set_permissions(&config.registry_dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(Server::start(config),Err(RemoteError::Io(e)) if e.kind()==io::ErrorKind::PermissionDenied)
        );
    }
}
