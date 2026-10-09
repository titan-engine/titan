//! The `screenshot` tool: captures the game's primary window as a PNG.
//!
//! Two paths, chosen by what `rpc.discover` advertises:
//!
//! 1. **Fast path:** `titan.screenshot` (or `titan.screenshot+watch`) from
//!    `TitanRemotePlugin`. We ask the game to write a PNG to a temp file we own,
//!    wait until a complete PNG exists at the returned path, read it (bounded),
//!    and delete our temp file. If the method returns a `token` instead of a
//!    `path`, `titan.screenshot_status` is polled with it.
//! 2. **Fallback:** spawn a BRP `Screenshot` entity and watch for
//!    `ScreenshotCaptured` with `world.observe+watch`. The whole image arrives as
//!    reflected JSON (slow and large), which we decode and encode as PNG here
//!    without depending on `bevy_render` or `bevy_image`.
//!
//! Everything is bounded by a deadline (`timeout_secs`, default 10, max 60) and
//! by size limits. Nothing is written to stdout.

use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use base64::Engine as _;
use bevy_ecs::entity::Entity;
use bevy_platform::collections::HashMap;
use bevy_remote::{
    builtin_methods::{
        BrpDespawnEntityParams, BrpObserveParams, BrpSpawnEntityParams, BrpSpawnEntityResponse,
        BRP_DESPAWN_COMPONENTS_METHOD, BRP_OBSERVE_METHOD, BRP_SPAWN_ENTITY_METHOD,
        RPC_DISCOVER_METHOD,
    },
    BrpError, BrpRequest,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

use crate::client::Client;

const TITAN_SCREENSHOT: &str = "titan.screenshot";
const TITAN_SCREENSHOT_WATCH: &str = "titan.screenshot+watch";
const TITAN_SCREENSHOT_STATUS: &str = "titan.screenshot_status";

const SCREENSHOT_COMPONENT: &str = "bevy_render::view::window::screenshot::Screenshot";
const SCREENSHOT_CAPTURED_EVENT: &str = "bevy_render::view::window::screenshot::ScreenshotCaptured";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TIMEOUT_SECS: f64 = 60.0;
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Largest PNG we read from disk or return to the agent.
const MAX_PNG_BYTES: u64 = 32 * 1024 * 1024;
/// Largest raw RGBA buffer accepted from the fallback (a 4K frame is ~33 MB).
const MAX_PIXEL_BYTES: u64 = 64 * 1024 * 1024;
/// Largest single SSE line accepted from the fallback. Reflected images are
/// JSON arrays of numbers, roughly 4 bytes of JSON per pixel byte.
const MAX_OBSERVE_LINE_BYTES: u64 = 256 * 1024 * 1024;
/// Largest single SSE line accepted from `titan.screenshot+watch`.
const MAX_TITAN_LINE_BYTES: u64 = 1024 * 1024;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const PNG_IEND: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];

/// Captures the primary window and returns a complete MCP `tools/call` result
/// with a single PNG image content block.
///
/// `args` may contain `timeout_secs` (number, `0 < t <= 60`, default 10).
pub fn capture(client: &Client, args: &Value) -> Result<Value, String> {
    let deadline = Instant::now() + timeout_from_args(args)?;
    let methods = discover_methods(client)?;

    let png = if methods.contains(TITAN_SCREENSHOT_WATCH) || methods.contains(TITAN_SCREENSHOT) {
        match capture_titan(client, &methods, deadline) {
            Ok(png) => png,
            Err(titan_err) => capture_observe(client, deadline).map_err(|fallback_err| {
                format!(
                    "{TITAN_SCREENSHOT} failed: {titan_err}. The world.observe fallback also failed: {fallback_err}"
                )
            })?,
        }
    } else {
        capture_observe(client, deadline)?
    };

    Ok(json!({
        "content": [{
            "type": "image",
            "mimeType": "image/png",
            "data": base64::engine::general_purpose::STANDARD.encode(&png),
        }],
        "isError": false,
    }))
}

fn timeout_from_args(args: &Value) -> Result<Duration, String> {
    match args.get("timeout_secs") {
        None | Some(Value::Null) => Ok(DEFAULT_TIMEOUT),
        Some(value) => match value.as_f64() {
            Some(secs) if secs > 0.0 && secs <= MAX_TIMEOUT_SECS => {
                Ok(Duration::from_secs_f64(secs))
            }
            _ => Err(format!(
                "`timeout_secs` must be a number greater than 0 and at most {MAX_TIMEOUT_SECS}"
            )),
        },
    }
}

fn discover_methods(client: &Client) -> Result<HashSet<String>, String> {
    let doc = client.call(RPC_DISCOVER_METHOD, None)?;
    Ok(doc
        .get("methods")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|method| method.get("name")?.as_str().map(str::to_owned))
        .collect())
}

fn timeout_error(deadline_context: &str) -> String {
    format!(
        "timed out waiting for {deadline_context}. The window must be visible (not minimized) for the game to render; pass a larger `timeout_secs` if the game is slow"
    )
}

// ---------------------------------------------------------------------------
// Fast path: titan.screenshot
// ---------------------------------------------------------------------------

fn capture_titan(
    client: &Client,
    methods: &HashSet<String>,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    let temp = TempFile::new();
    let params = json!({ "path": temp.path });

    let path = if methods.contains(TITAN_SCREENSHOT_WATCH) {
        watch(
            client,
            TITAN_SCREENSHOT_WATCH,
            params,
            deadline,
            MAX_TITAN_LINE_BYTES,
            |result: Value| Ok(result_path(&result)),
        )?
    } else {
        let result = client.call(TITAN_SCREENSHOT, Some(params))?;
        match (result_path(&result), result.get("token")) {
            (Some(path), _) => path,
            (None, Some(token)) if methods.contains(TITAN_SCREENSHOT_STATUS) => {
                poll_status(client, token, deadline)?
            }
            _ => {
                return Err(format!(
                    "{TITAN_SCREENSHOT} returned neither a `path` nor a pollable `token`: {}",
                    truncate(&result.to_string(), 200)
                ))
            }
        }
    };

    wait_for_png(&path, deadline)
}

/// Extracts `path` from a `titan.screenshot*` result (`{ "path": ... }`).
fn result_path(result: &Value) -> Option<PathBuf> {
    result
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn poll_status(client: &Client, token: &Value, deadline: Instant) -> Result<PathBuf, String> {
    loop {
        let status = client.call(TITAN_SCREENSHOT_STATUS, Some(json!({ "token": token })))?;
        if let Some(path) = result_path(&status) {
            return Ok(path);
        }
        if Instant::now() >= deadline {
            return Err(timeout_error(TITAN_SCREENSHOT_STATUS));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Waits until `path` holds a complete PNG (the game writes it asynchronously),
/// then returns its bytes.
fn wait_for_png(path: &Path, deadline: Instant) -> Result<Vec<u8>, String> {
    if !path.is_absolute() {
        return Err(format!(
            "the game returned a relative screenshot path `{}`, which can't be resolved here",
            path.display()
        ));
    }
    loop {
        if let Ok(meta) = fs::metadata(path) {
            if meta.len() > MAX_PNG_BYTES {
                return Err(format!(
                    "screenshot `{}` is {} bytes, over the {MAX_PNG_BYTES} byte limit",
                    path.display(),
                    meta.len()
                ));
            }
            if let Some(png) = read_bounded(path)?.filter(|bytes| is_complete_png(bytes)) {
                return Ok(png);
            }
        }
        if Instant::now() >= deadline {
            return Err(timeout_error(&format!(
                "a complete PNG at `{}`",
                path.display()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Reads at most `MAX_PNG_BYTES`. `Ok(None)` if the file vanished or is too big.
fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let Ok(file) = fs::File::open(path) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(MAX_PNG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading screenshot `{}` failed: {e}", path.display()))?;
    Ok((bytes.len() as u64 <= MAX_PNG_BYTES).then_some(bytes))
}

/// Cheap completeness check: PNG signature at the start and an `IEND` chunk at the end.
fn is_complete_png(bytes: &[u8]) -> bool {
    bytes.len() >= PNG_SIGNATURE.len() + PNG_IEND.len()
        && bytes.starts_with(&PNG_SIGNATURE)
        && bytes.ends_with(&PNG_IEND)
}

/// A temp file path we own; removed on drop, whether capture succeeded or not.
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "titan_mcp-screenshot-{}-{nanos}-{}.png",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        Self {
            path: std::env::temp_dir().join(name),
        }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Fallback: Screenshot entity + world.observe+watch
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CapturedEvent {
    image: ReflectedImage,
}

/// The fields we need from `bevy_image::SerializedImage`.
#[derive(Deserialize)]
struct ReflectedImage {
    data: Option<Vec<u8>>,
    texture_descriptor: ReflectedTextureDescriptor,
}

#[derive(Deserialize)]
struct ReflectedTextureDescriptor {
    size: ReflectedExtent,
    format: String,
}

#[derive(Deserialize)]
struct ReflectedExtent {
    width: u32,
    height: u32,
    #[serde(default = "one")]
    depth_or_array_layers: u32,
}

fn one() -> u32 {
    1
}

fn capture_observe(client: &Client, deadline: Instant) -> Result<Vec<u8>, String> {
    let spawn = BrpSpawnEntityParams {
        components: HashMap::from([(
            SCREENSHOT_COMPONENT.to_owned(),
            json!({ "Window": "Primary" }),
        )]),
    };
    let spawned = client
        .call(BRP_SPAWN_ENTITY_METHOD, Some(to_params(&spawn)?))
        .map_err(|e| {
            format!(
                "couldn't spawn a `Screenshot` entity ({e}). Screenshots need a game with a window and bevy_render; add TitanRemotePlugin for faster screenshots"
            )
        })?;
    let BrpSpawnEntityResponse { entity } = serde_json::from_value(spawned)
        .map_err(|e| format!("unexpected {BRP_SPAWN_ENTITY_METHOD} result: {e}"))?;

    // Bevy despawns the screenshot entity itself once captured. If we bail out
    // before that, despawn it so it doesn't linger.
    let mut guard = DespawnGuard {
        client,
        entity: Some(entity),
    };
    let observe = BrpObserveParams {
        event: SCREENSHOT_CAPTURED_EVENT.to_owned(),
        entity: Some(entity),
    };
    let image = watch(
        client,
        BRP_OBSERVE_METHOD,
        to_params(&observe)?,
        deadline,
        MAX_OBSERVE_LINE_BYTES,
        |events: Vec<CapturedEvent>| Ok(events.into_iter().next().map(|event| event.image)),
    )?;
    guard.entity = None;

    encode_png(image)
}

struct DespawnGuard<'a> {
    client: &'a Client,
    entity: Option<Entity>,
}

impl Drop for DespawnGuard<'_> {
    fn drop(&mut self) {
        if let Some(entity) = self.entity.take()
            && let Ok(params) = to_params(&BrpDespawnEntityParams { entity })
        {
            let _ = self
                .client
                .call(BRP_DESPAWN_COMPONENTS_METHOD, Some(params));
        }
    }
}

fn to_params(params: &impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(params).map_err(|e| format!("encoding BRP params failed: {e}"))
}

/// Converts a reflected 8-bit RGBA/BGRA image to an opaque RGB PNG.
fn encode_png(image: ReflectedImage) -> Result<Vec<u8>, String> {
    let ReflectedExtent {
        width,
        height,
        depth_or_array_layers,
    } = image.texture_descriptor.size;
    let format = image.texture_descriptor.format;
    let bgra = match format.as_str() {
        "rgba8unorm" | "rgba8unorm-srgb" => false,
        "bgra8unorm" | "bgra8unorm-srgb" => true,
        _ => {
            return Err(format!(
                "screenshot texture format `{format}` isn't supported; expected an 8-bit RGBA or BGRA surface"
            ))
        }
    };
    let expected = u64::from(width) * u64::from(height) * 4;
    if width == 0 || height == 0 || depth_or_array_layers != 1 || expected > MAX_PIXEL_BYTES {
        return Err(format!(
            "screenshot size {width}x{height}x{depth_or_array_layers} is empty or over the {MAX_PIXEL_BYTES} byte limit"
        ));
    }
    let data = image.data.unwrap_or_default();
    if data.len() as u64 != expected {
        return Err(format!(
            "screenshot pixel buffer is {} bytes, expected {expected} for {width}x{height} {format}",
            data.len()
        ));
    }

    let rgb: Vec<u8> = data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| {
            if bgra {
                [px[2], px[1], px[0]]
            } else {
                [px[0], px[1], px[2]]
            }
        })
        .collect();

    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| {
            writer.write_image_data(&rgb)?;
            writer.finish()
        })
        .map_err(|e| format!("encoding screenshot PNG failed: {e}"))?;
    if png.len() as u64 > MAX_PNG_BYTES {
        return Err(format!(
            "screenshot PNG is {} bytes, over the {MAX_PNG_BYTES} byte limit",
            png.len()
        ));
    }
    Ok(png)
}

// ---------------------------------------------------------------------------
// Bounded streaming (`+watch`) request
// ---------------------------------------------------------------------------

/// A JSON-RPC response frame with a typed `result`.
#[derive(Deserialize)]
struct Frame<T> {
    result: Option<T>,
    error: Option<BrpError>,
}

/// Sends a BRP `+watch` request and reads SSE `data:` lines until `on_result`
/// returns `Some`, an error frame arrives, the stream ends, or `deadline` passes.
///
/// Each line is capped at `max_line` bytes and parsed straight into `T` (no
/// intermediate `Value`), which matters for multi-megabyte reflected images.
fn watch<T: DeserializeOwned, R>(
    client: &Client,
    method: &str,
    params: Value,
    deadline: Instant,
    max_line: u64,
    mut on_result: impl FnMut(T) -> Result<Option<R>, String>,
) -> Result<R, String> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(timeout_error(method));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT.min(remaining)))
        .timeout_global(Some(remaining))
        .build()
        .into();
    let request = BrpRequest {
        method: method.to_owned(),
        id: Some(json!(1)),
        params: Some(params),
    };
    let response = agent.post(client.url()).send_json(&request).map_err(|e| match e {
        ureq::Error::Timeout(_) => timeout_error(method),
        e => format!(
            "BRP {method} at {} failed: {e}. Is the game running with RemotePlugin and RemoteHttpPlugin?",
            client.url()
        ),
    })?;
    if !response.status().is_success() {
        return Err(format!("BRP {method} returned HTTP {}", response.status()));
    }

    let mut reader = BufReader::new(
        response
            .into_body()
            .into_with_config()
            .limit(u64::MAX)
            .reader(),
    );
    let mut line = Vec::new();
    loop {
        if !read_line_bounded(&mut reader, &mut line, max_line)
            .map_err(|e| stream_error(method, &e, deadline))?
        {
            return Err(format!("BRP {method} stream ended before a result arrived"));
        }
        let trimmed = line.trim_ascii();
        // SSE frames are `data: {...}`; a plain JSON body (e.g. a request-level
        // error) is accepted too. Blank lines and other SSE fields are skipped.
        let json = match trimmed.strip_prefix(b"data:") {
            Some(rest) => rest.trim_ascii_start(),
            None if trimmed.starts_with(b"{") => trimmed,
            None => continue,
        };
        let frame: Frame<T> = serde_json::from_slice(json)
            .map_err(|e| format!("unexpected BRP {method} response: {e}"))?;
        if let Some(BrpError { code, message, .. }) = frame.error {
            return Err(format!("BRP {method} error {code}: {message}"));
        }
        if let Some(result) = frame.result
            && let Some(done) = on_result(result)?
        {
            return Ok(done);
        }
        if Instant::now() >= deadline {
            return Err(timeout_error(method));
        }
    }
}

fn stream_error(method: &str, error: &str, deadline: Instant) -> String {
    if Instant::now() >= deadline {
        timeout_error(method)
    } else {
        format!("reading BRP {method} stream failed: {error}")
    }
}

/// Reads one `\n`-terminated line into `buf`, refusing lines over `max` bytes.
/// Returns `Ok(false)` at end of stream.
fn read_line_bounded(
    reader: &mut impl BufRead,
    buf: &mut Vec<u8>,
    max: u64,
) -> Result<bool, String> {
    buf.clear();
    let read = reader
        .take(max + 1)
        .read_until(b'\n', buf)
        .map_err(|e| e.to_string())?;
    if read == 0 {
        return Ok(false);
    }
    if buf.len() as u64 > max && !buf.ends_with(b"\n") {
        return Err(format!("a response line exceeded the {max} byte limit"));
    }
    Ok(true)
}

fn truncate(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use std::{
        io::Write,
        net::{TcpListener, TcpStream},
        sync::Mutex,
    };

    /// What the stub BRP server sends back for one request.
    enum Reply {
        Json(Value),
        /// SSE `data:` frames, then hold the connection open for `hold`.
        Sse(Vec<Value>, Duration),
    }

    type Calls = Arc<Mutex<Vec<(String, Value)>>>;

    /// A minimal HTTP/1.1 BRP server on a free loopback port. Each connection
    /// is handled on its own thread so streams don't block other requests.
    fn stub(handler: impl Fn(&str, &Value) -> Reply + Send + Sync + 'static) -> (Client, Calls) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let calls: Calls = Arc::default();
        let handler = Arc::new(handler);
        let server_calls = calls.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (handler, calls) = (handler.clone(), server_calls.clone());
                thread::spawn(move || serve(stream, &*handler, &calls));
            }
        });
        (Client::new(&url).unwrap(), calls)
    }

    fn serve(mut stream: TcpStream, handler: &dyn Fn(&str, &Value) -> Reply, calls: &Calls) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut content_length = 0;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                return;
            }
            if header == "\r\n" {
                break;
            }
            if let Some((name, value)) = header.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                content_length = value.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();
        let method = request["method"].as_str().unwrap().to_owned();
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        calls.lock().unwrap().push((method.clone(), params.clone()));
        match handler(&method, &params) {
            Reply::Json(result) => {
                let body =
                    json!({ "jsonrpc": "2.0", "id": request["id"], "result": result }).to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
            Reply::Sse(frames, hold) => {
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
                );
                for frame in frames {
                    let _ = write!(stream, "data: {frame}\n\n");
                }
                let _ = stream.flush();
                thread::sleep(hold);
            }
        }
    }

    fn discover(methods: &[&str]) -> Reply {
        let methods: Vec<Value> = methods.iter().map(|name| json!({ "name": name })).collect();
        Reply::Json(json!({ "openrpc": "1.3.2", "methods": methods }))
    }

    fn tiny_png() -> Vec<u8> {
        let image = ReflectedImage {
            data: Some(vec![10, 20, 30, 255]),
            texture_descriptor: ReflectedTextureDescriptor {
                size: ReflectedExtent {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                format: "rgba8unorm".into(),
            },
        };
        encode_png(image).unwrap()
    }

    fn decode(result: &Value) -> (png::OutputInfo, Vec<u8>) {
        assert_eq!(result["isError"], false);
        let block = &result["content"][0];
        assert_eq!(block["type"], "image");
        assert_eq!(block["mimeType"], "image/png");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(block["data"].as_str().unwrap())
            .unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        pixels.truncate(info.buffer_size());
        (info, pixels)
    }

    fn methods(calls: &Calls) -> Vec<String> {
        calls
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect()
    }

    #[test]
    fn titan_screenshot_reads_and_removes_the_file() {
        let written = Arc::new(Mutex::new(PathBuf::new()));
        let seen = written.clone();
        let (client, calls) = stub(move |method, params| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT, BRP_OBSERVE_METHOD]),
            TITAN_SCREENSHOT => {
                let path = PathBuf::from(params["path"].as_str().unwrap());
                // Simulate the asynchronous write: partial file first, then complete.
                let png = tiny_png();
                fs::write(&path, &png[..png.len() - 4]).unwrap();
                let finish = path.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(100));
                    fs::write(finish, tiny_png()).unwrap();
                });
                *seen.lock().unwrap() = path.clone();
                Reply::Json(json!({ "path": path }))
            }
            _ => panic!("unexpected {method}"),
        });

        let result = capture(&client, &json!({})).unwrap();
        let (info, pixels) = decode(&result);
        assert_eq!((info.width, info.height), (1, 1));
        assert_eq!(pixels, [10, 20, 30]);
        assert_eq!(methods(&calls), [RPC_DISCOVER_METHOD, TITAN_SCREENSHOT]);
        assert!(!written.lock().unwrap().exists(), "temp file not removed");
    }

    #[test]
    fn titan_screenshot_watch_and_token_status() {
        let (client, calls) = stub(|method, params| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT_WATCH]),
            TITAN_SCREENSHOT_WATCH => {
                let path = params["path"].as_str().unwrap();
                fs::write(path, tiny_png()).unwrap();
                Reply::Sse(
                    vec![
                        json!({ "jsonrpc": "2.0", "id": 1, "result": { "pending": true } }),
                        json!({ "jsonrpc": "2.0", "id": 1, "result": { "path": path } }),
                    ],
                    Duration::from_secs(5),
                )
            }
            _ => panic!("unexpected {method}"),
        });
        decode(&capture(&client, &json!({})).unwrap());
        assert_eq!(
            methods(&calls),
            [RPC_DISCOVER_METHOD, TITAN_SCREENSHOT_WATCH]
        );

        let path = Arc::new(Mutex::new(String::new()));
        let (client, calls) = stub(move |method, params| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT, TITAN_SCREENSHOT_STATUS]),
            TITAN_SCREENSHOT => {
                *path.lock().unwrap() = params["path"].as_str().unwrap().to_owned();
                Reply::Json(json!({ "token": 7 }))
            }
            TITAN_SCREENSHOT_STATUS => {
                assert_eq!(params["token"], 7);
                let path = path.lock().unwrap().clone();
                fs::write(&path, tiny_png()).unwrap();
                Reply::Json(json!({ "path": path }))
            }
            _ => panic!("unexpected {method}"),
        });
        decode(&capture(&client, &json!({})).unwrap());
        assert_eq!(
            methods(&calls),
            [
                RPC_DISCOVER_METHOD,
                TITAN_SCREENSHOT,
                TITAN_SCREENSHOT_STATUS
            ]
        );
    }

    #[test]
    fn fallback_decodes_reflected_bgra_image() {
        let (client, calls) = stub(|method, params| match method {
            RPC_DISCOVER_METHOD => discover(&[BRP_SPAWN_ENTITY_METHOD, BRP_OBSERVE_METHOD]),
            BRP_SPAWN_ENTITY_METHOD => {
                assert_eq!(
                    params["components"][SCREENSHOT_COMPONENT]["Window"],
                    "Primary"
                );
                Reply::Json(json!({ "entity": 42 }))
            }
            BRP_OBSERVE_METHOD => {
                assert_eq!(params["event"], SCREENSHOT_CAPTURED_EVENT);
                assert_eq!(params["entity"], 42);
                // Shape of a reflected `ScreenshotCaptured` (`SerializedImage`).
                let event = json!({
                    "entity": 42,
                    "image": {
                        "data": [1, 2, 3, 255, 4, 5, 6, 0],
                        "data_order": "LayerMajor",
                        "texture_descriptor": {
                            "label": null,
                            "size": { "width": 2, "height": 1, "depth_or_array_layers": 1 },
                            "mip_level_count": 1,
                            "sample_count": 1,
                            "dimension": "d2",
                            "format": "bgra8unorm-srgb",
                            "usage": 1,
                            "view_formats": []
                        },
                        "sampler": "Default",
                        "texture_view_descriptor": null
                    }
                });
                Reply::Sse(
                    vec![json!({ "jsonrpc": "2.0", "id": 1, "result": [event] })],
                    Duration::from_secs(5),
                )
            }
            _ => panic!("unexpected {method}"),
        });

        let (info, pixels) = decode(&capture(&client, &json!({})).unwrap());
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(pixels, [3, 2, 1, 6, 5, 4]);
        // Bevy despawns the entity after capture; we must not.
        assert_eq!(
            methods(&calls),
            [
                RPC_DISCOVER_METHOD,
                BRP_SPAWN_ENTITY_METHOD,
                BRP_OBSERVE_METHOD
            ]
        );
    }

    #[test]
    fn fallback_errors_and_timeouts_despawn_the_entity() {
        let (client, calls) = stub(|method, _| match method {
            RPC_DISCOVER_METHOD => discover(&[]),
            BRP_SPAWN_ENTITY_METHOD => Reply::Json(json!({ "entity": 5 })),
            BRP_OBSERVE_METHOD => Reply::Sse(
                vec![
                    json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -23402, "message": "Unknown event type" } }),
                ],
                Duration::ZERO,
            ),
            BRP_DESPAWN_COMPONENTS_METHOD => Reply::Json(Value::Null),
            _ => panic!("unexpected {method}"),
        });
        let err = capture(&client, &json!({})).unwrap_err();
        assert!(err.contains("Unknown event type"), "{err}");
        let calls = calls.lock().unwrap();
        assert_eq!(calls.last().unwrap().0, BRP_DESPAWN_COMPONENTS_METHOD);
        assert_eq!(calls.last().unwrap().1["entity"], 5);
        drop(calls);

        // A stream that never delivers must time out promptly and clean up.
        let (client, calls) = stub(|method, _| match method {
            RPC_DISCOVER_METHOD => discover(&[]),
            BRP_SPAWN_ENTITY_METHOD => Reply::Json(json!({ "entity": 6 })),
            BRP_OBSERVE_METHOD => Reply::Sse(vec![], Duration::from_secs(10)),
            BRP_DESPAWN_COMPONENTS_METHOD => Reply::Json(Value::Null),
            _ => panic!("unexpected {method}"),
        });
        let start = Instant::now();
        let err = capture(&client, &json!({ "timeout_secs": 0.3 })).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(
            methods(&calls).last().unwrap(),
            BRP_DESPAWN_COMPONENTS_METHOD
        );
    }

    #[test]
    fn titan_failure_falls_back_and_reports_both() {
        let (client, _) = stub(|method, _| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT]),
            TITAN_SCREENSHOT => Reply::Json(json!({ "unexpected": true })),
            BRP_SPAWN_ENTITY_METHOD => Reply::Json(json!({ "entity": 1 })),
            BRP_OBSERVE_METHOD => Reply::Sse(
                vec![
                    json!({ "jsonrpc": "2.0", "id": 1, "result": [{ "entity": 1, "image": {
                    "data": [0, 0, 0],
                    "texture_descriptor": { "size": { "width": 1, "height": 1 }, "format": "rgba8unorm" }
                } }] }),
                ],
                Duration::ZERO,
            ),
            _ => panic!("unexpected {method}"),
        });
        let err = capture(&client, &json!({})).unwrap_err();
        assert!(err.contains("neither a `path`"), "{err}");
        assert!(err.contains("pixel buffer is 3 bytes"), "{err}");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(timeout_from_args(&json!({ "timeout_secs": 0 })).is_err());
        assert!(timeout_from_args(&json!({ "timeout_secs": 61 })).is_err());
        assert!(timeout_from_args(&json!({ "timeout_secs": "5" })).is_err());
        assert_eq!(timeout_from_args(&json!({})).unwrap(), DEFAULT_TIMEOUT);

        let png = tiny_png();
        assert!(is_complete_png(&png));
        assert!(!is_complete_png(&png[..png.len() - 1]));
        assert!(!is_complete_png(b"not a png"));

        let mut buf = Vec::new();
        let mut long = BufReader::new(&b"0123456789\nok\n"[..]);
        assert!(read_line_bounded(&mut long, &mut buf, 5).is_err());
        let mut short = BufReader::new(&b"0123\nok"[..]);
        assert!(read_line_bounded(&mut short, &mut buf, 5).unwrap());
        assert_eq!(buf, b"0123\n");
        assert!(read_line_bounded(&mut short, &mut buf, 5).unwrap());
        assert!(!read_line_bounded(&mut short, &mut buf, 5).unwrap());

        let format = |format: &str, data: Vec<u8>, width: u32| {
            encode_png(ReflectedImage {
                data: Some(data),
                texture_descriptor: ReflectedTextureDescriptor {
                    size: ReflectedExtent {
                        width,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    format: format.into(),
                },
            })
        };
        assert!(format("rgba16float", vec![0; 8], 1).is_err());
        assert!(format("rgba8unorm", vec![0; 4], 0).is_err());
        assert!(format("rgba8unorm", vec![0; 4], u32::MAX).is_err());

        assert!(wait_for_png(Path::new("relative.png"), Instant::now()).is_err());
    }
}
