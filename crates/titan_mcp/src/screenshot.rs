//! The `screenshot` tool: captures the game's primary window as a PNG.
//!
//! Two paths, chosen by what `rpc.discover` advertises:
//!
//! 1. **Fast path:** `titan.screenshot` (or `titan.screenshot+watch`) from
//!    `TitanRemotePlugin`. We ask the game to publish the PNG to an exact path
//!    inside a private temp directory, then securely open the published regular
//!    file (not a pre-publication handle). Any other returned path is rejected.
//!    If the method returns a `token`, `titan.screenshot_status` is polled with
//!    it. Atomic publication and legacy in-place writes are both supported.
//! 2. **Fallback:** spawn an empty BRP entity, register a `ScreenshotCaptured`
//!    observer, then insert `Screenshot`. The whole image arrives as
//!    reflected JSON (slow and large), which we decode and encode as PNG here
//!    without depending on `bevy_render` or `bevy_image`.
//!
//! Every BRP call shares one deadline (`timeout_secs`, default 10, max 60), and
//! all reads are size-limited. Nothing is written to stdout.

use core::time::Duration;
use std::{
    collections::HashSet,
    fs,
    io::{self, BufRead, BufReader, Cursor, Read},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Instant,
};

use base64::Engine as _;
use bevy_ecs::{entity::Entity, observer::ObservedBy};
use bevy_platform::collections::HashMap;
use bevy_remote::{
    builtin_methods::{
        BrpDespawnEntityParams, BrpInsertComponentsParams, BrpListComponentsParams,
        BrpListComponentsResponse, BrpObserveParams, BrpSpawnEntityParams, BrpSpawnEntityResponse,
        BRP_DESPAWN_COMPONENTS_METHOD, BRP_INSERT_COMPONENTS_METHOD, BRP_LIST_COMPONENTS_METHOD,
        BRP_OBSERVE_METHOD, BRP_SPAWN_ENTITY_METHOD, RPC_DISCOVER_METHOD,
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
/// Extra time best-effort cleanup (despawning a fallback `Screenshot` entity)
/// may take, even when the user's deadline has already passed.
const CLEANUP_BUDGET: Duration = Duration::from_millis(100);

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
/// `args` may contain `timeout_secs` (number, `0 < t <= 60`, default 10). The
/// timeout covers every BRP call made here; only best-effort cleanup of a
/// fallback `Screenshot` entity may run up to 100 ms past it.
pub fn capture(client: &Client, args: &Value) -> Result<Value, String> {
    let deadline = Instant::now() + timeout_from_args(args)?;
    let methods = discover_methods(client, deadline)?;

    let png = if methods.contains(TITAN_SCREENSHOT_WATCH) || methods.contains(TITAN_SCREENSHOT) {
        // Respect the advertised backend's operational errors, including its
        // limit on stuck renderer readbacks. Raw BRP fallback would bypass that
        // accounting by spawning additional Screenshot entities directly.
        capture_titan(client, &methods, deadline)?
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

fn discover_methods(client: &Client, deadline: Instant) -> Result<HashSet<String>, String> {
    let doc = client.call_with_deadline(RPC_DISCOVER_METHOD, None, deadline)?;
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
    // TitanRemotePlugin publishes by rename. A precreated NamedTempFile handle
    // would still refer to the old inode after publication and read zero bytes.
    // Reserve the directory instead, and open the destination only after the
    // RPC completes. The directory guard also removes staging files on failure.
    let destination = ScreenshotDestination::new()?;
    let requested = &destination.path;
    let params = json!({ "path": requested });

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
        let result = client.call_with_deadline(TITAN_SCREENSHOT, Some(params), deadline)?;
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
    if path != *requested {
        return Err(format!(
            "{TITAN_SCREENSHOT} wrote to `{}` instead of the requested `{requested}`; refusing to read any other destination",
            truncate(&path, 200)
        ));
    }

    wait_for_png(&destination, deadline)
}

const SCREENSHOT_FILE: &str = "capture.png";

/// Pins the private parent directory while the game publishes the file.
///
/// The temporary directory's ancestors are the local, trusted temp-directory
/// hierarchy. The game controls the leaf and can rename our immediate parent:
/// Unix reads are relative to a directory handle, not a racy full-path lookup;
/// Windows denies write/delete sharing of the parent (including conversion to
/// a reparse point). Leaf metadata
/// is checked on the opened handle, never as a check-then-open security barrier.
struct ScreenshotDestination {
    // Field order matters on Windows: close the directory before removing it.
    directory: fs::File,
    _temp: tempfile::TempDir,
    path: String,
}

impl ScreenshotDestination {
    fn new() -> Result<Self, String> {
        let temp = tempfile::Builder::new()
            .prefix("titan_mcp-screenshot-")
            .tempdir()
            .map_err(|e| format!("couldn't create a private screenshot directory: {e}"))?;
        let parent = std::path::absolute(temp.path()).map_err(|e| e.to_string())?;
        let path = parent
            .join(SCREENSHOT_FILE)
            .to_str()
            .ok_or("the temp directory path isn't valid UTF-8")?
            .to_owned();
        let directory = open_directory(&parent)
            .map_err(|e| format!("couldn't secure the screenshot directory: {e}"))?;
        Ok(Self {
            directory,
            _temp: temp,
            path,
        })
    }

    fn open(&self) -> io::Result<fs::File> {
        #[cfg(unix)]
        let file = {
            use rustix::fs::{openat, Mode, OFlags};
            fs::File::from(openat(
                &self.directory,
                SCREENSHOT_FILE,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                Mode::empty(),
            )?)
        };
        #[cfg(windows)]
        let file = {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_FLAG_OPEN_REPARSE_POINT (0x00200000) opens the link itself,
            // rather than its target, even if swapped in just before this call.
            // Do not share DELETE while the file's metadata/bytes are inspected.
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0x00000001 | 0x00000002) // FILE_SHARE_READ | FILE_SHARE_WRITE
                .custom_flags(0x00200000)
                .open(&self.path)?
        };
        #[cfg(not(any(unix, windows)))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "secure screenshot file opening is unsupported on this platform",
        ));

        #[cfg(any(unix, windows))]
        {
            let metadata = file.metadata()?;
            reject_reparse_point(&metadata)?;
            if !metadata.is_file() {
                return Err(io::Error::other("screenshot is not a regular file"));
            }
            Ok(file)
        }
    }
}

fn open_directory(path: &std::path::Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    {
        use rustix::fs::{open, Mode, OFlags};
        Ok(fs::File::from(open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = fs::OpenOptions::new()
            .read(true)
            // FILE_SHARE_READ only: omitting DELETE prevents replacement;
            // omitting WRITE prevents opening it with GENERIC_WRITE to convert
            // the held directory to a reparse point (FSCTL_SET_REPARSE_POINT).
            // Creating/renaming children does not require a writable handle to
            // this parent, so the server can still atomically publish the PNG.
            .share_mode(0x00000001)
            // FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT.
            .custom_flags(0x02000000 | 0x00200000)
            .open(path)?;
        let metadata = directory.metadata()?;
        reject_reparse_point(&metadata)?;
        if !metadata.is_dir() {
            return Err(io::Error::other("screenshot parent is not a directory"));
        }
        Ok(directory)
    }
    #[cfg(not(any(unix, windows)))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure screenshot file opening is unsupported on this platform",
    ))
}

fn reject_reparse_point(metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x00000400 != 0 {
            // FILE_ATTRIBUTE_REPARSE_POINT: includes symlinks and junctions.
            return Err(io::Error::other("screenshot path is a reparse point"));
        }
    }
    #[cfg(not(windows))]
    let _ = metadata;
    Ok(())
}

/// Extracts `path` from a `titan.screenshot*` result (`{ "path": ... }`).
fn result_path(result: &Value) -> Option<String> {
    result
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
}

fn poll_status(client: &Client, token: &Value, deadline: Instant) -> Result<String, String> {
    loop {
        let status = client.call_with_deadline(
            TITAN_SCREENSHOT_STATUS,
            Some(json!({ "token": token })),
            deadline,
        )?;
        if status.get("pending") != Some(&Value::Bool(true)) {
            if let Some(path) = result_path(&status) {
                return Ok(path);
            }
            if status.get("pending") == Some(&Value::Bool(false)) {
                return Err(format!(
                    "{TITAN_SCREENSHOT_STATUS} completed without a `path`"
                ));
            }
        }
        if Instant::now() + POLL_INTERVAL >= deadline {
            return Err(timeout_error(TITAN_SCREENSHOT_STATUS));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Reopens on each poll: atomic publication may replace a partial legacy file.
/// Only an opened, verified regular file is read, with byte and deadline limits.
fn wait_for_png(destination: &ScreenshotDestination, deadline: Instant) -> Result<Vec<u8>, String> {
    let path = &destination.path;
    loop {
        if Instant::now() >= deadline {
            return Err(timeout_error("a complete screenshot PNG"));
        }
        let bytes = match destination.open() {
            Ok(file) => read_png(file, path, deadline)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(format!("securely opening screenshot `{path}` failed: {e}")),
        };
        // Cheap check first so a half-written file isn't decoded every poll.
        if looks_complete(&bytes) {
            validate_png(&bytes, deadline)
                .map_err(|e| format!("screenshot `{path}` isn't a valid PNG: {e}"))?;
            if Instant::now() >= deadline {
                return Err(timeout_error("decoding the screenshot PNG"));
            }
            return Ok(bytes);
        }
        if Instant::now() + POLL_INTERVAL >= deadline {
            return Err(timeout_error(&format!("a complete PNG at `{path}`")));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn read_png(mut file: fs::File, path: &str, deadline: Instant) -> Result<Vec<u8>, String> {
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_PNG_BYTES {
        return Err(format!(
            "screenshot `{path}` is over the {MAX_PNG_BYTES} byte limit"
        ));
    }
    let mut bytes = Vec::new();
    let mut chunk = [0; 16 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(timeout_error("reading the screenshot PNG"));
        }
        // Read one extra byte to detect a file growing past the limit.
        let remaining = (MAX_PNG_BYTES + 1 - bytes.len() as u64) as usize;
        let capacity = chunk.len().min(remaining);
        let count = file
            .read(&mut chunk[..capacity])
            .map_err(|e| format!("reading screenshot `{path}` failed: {e}"))?;
        if bytes.len() as u64 + count as u64 > MAX_PNG_BYTES {
            return Err(format!(
                "screenshot `{path}` is over the {MAX_PNG_BYTES} byte limit"
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if count == 0 {
            return Ok(bytes);
        }
    }
}

/// PNG signature at the start and an `IEND` chunk at the end.
fn looks_complete(bytes: &[u8]) -> bool {
    bytes.len() >= PNG_SIGNATURE.len() + PNG_IEND.len()
        && bytes.starts_with(&PNG_SIGNATURE)
        && bytes.ends_with(&PNG_IEND)
}

/// Fully decodes an in-memory PNG (checksums included) within the pixel limit,
/// so a spoofed signature + `IEND` isn't handed to the agent as an image.
fn validate_png(bytes: &[u8], deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        return Err(timeout_error("decoding the screenshot PNG"));
    }
    let limits = png::Limits {
        bytes: MAX_PIXEL_BYTES as usize,
    };
    let mut reader = png::Decoder::new_with_limits(Cursor::new(bytes), limits)
        .read_info()
        .map_err(|e| e.to_string())?;
    let info = reader.info();
    if info.animation_control.is_some() {
        return Err("animated PNG screenshots aren't supported".to_owned());
    }
    // Bound dimensions independently of the encoded color depth: a grayscale
    // or indexed PNG must not bypass the RGBA-sized pixel budget.
    u64::from(info.width)
        .checked_mul(u64::from(info.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&bytes| bytes > 0 && bytes <= MAX_PIXEL_BYTES)
        .ok_or_else(|| format!("image dimensions exceed the {MAX_PIXEL_BYTES} byte limit"))?;
    reader
        .output_buffer_size()
        .filter(|&size| size as u64 <= MAX_PIXEL_BYTES)
        .ok_or_else(|| format!("decoded image is over the {MAX_PIXEL_BYTES} byte limit"))?;
    // Decode every row (including interlaced passes) without allocating another
    // whole-frame buffer. Check the shared deadline between bounded rows. PNG
    // decoding and OS file I/O are synchronous, not forcibly preemptible.
    loop {
        if Instant::now() >= deadline {
            return Err(timeout_error("decoding the screenshot PNG"));
        }
        if reader.next_row().map_err(|e| e.to_string())?.is_none() {
            break;
        }
    }
    reader.finish().map_err(|e| e.to_string())?;
    if Instant::now() >= deadline {
        return Err(timeout_error("decoding the screenshot PNG"));
    }
    Ok(())
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
    // No capture may start until the entity-scoped observer is registered.
    let spawn = BrpSpawnEntityParams {
        components: HashMap::default(),
    };
    let spawned = client
        .call_with_deadline(BRP_SPAWN_ENTITY_METHOD, Some(to_params(&spawn)?), deadline)
        .map_err(|e| {
            format!(
                "couldn't spawn a `Screenshot` entity ({e}). Screenshots need a game with a window and bevy_render; add TitanRemotePlugin for faster screenshots"
            )
        })?;
    let BrpSpawnEntityResponse { entity } = serde_json::from_value(spawned)
        .map_err(|e| format!("unexpected {BRP_SPAWN_ENTITY_METHOD} result: {e}"))?;

    // Bevy despawns the screenshot entity itself once captured. If we bail out
    // before that, despawn it so it doesn't linger.
    let guard = DespawnGuard {
        client,
        entity: Some(entity),
    };
    let observe = BrpObserveParams {
        event: SCREENSHOT_CAPTURED_EVENT.to_owned(),
        entity: Some(entity),
    };
    let response = open_watch(client, BRP_OBSERVE_METHOD, to_params(&observe)?, deadline)?;
    // Read concurrently so registration errors (including unknown event types)
    // aren't hidden behind a readiness timeout. Joining the deadline-bounded
    // worker on every exit prevents repeated failed captures accumulating idle
    // readers. ureq cannot cancel an in-flight body read, so an early failure
    // may wait for the remaining budget, but entity cleanup starts immediately.
    thread::scope(move |scope| {
        let mut guard = guard;
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = thread::Builder::new()
            .name("titan_mcp screenshot observer".to_owned())
            .spawn_scoped(scope, move || {
                let result = read_watch(
                    response,
                    BRP_OBSERVE_METHOD,
                    deadline,
                    MAX_OBSERVE_LINE_BYTES,
                    |events: Vec<CapturedEvent>| {
                        Ok(events.into_iter().next().map(|event| event.image))
                    },
                );
                let _ = sender.send(result);
            })
            .map_err(|e| format!("couldn't start the screenshot observer reader: {e}"))?;
        let result = (|| {
            wait_for_observer(client, entity, &receiver, deadline)?;
            client.call_with_deadline(
                BRP_INSERT_COMPONENTS_METHOD,
                Some(to_params(&BrpInsertComponentsParams {
                    entity,
                    components: HashMap::from([(
                        SCREENSHOT_COMPONENT.to_owned(),
                        json!({ "Window": "Primary" }),
                    )]),
                })?),
                deadline,
            )?;
            let image = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| timeout_error(BRP_OBSERVE_METHOD))??;
            guard.entity = None;
            Ok(image)
        })();
        drop(receiver);
        drop(guard);
        reader
            .join()
            .map_err(|_| "screenshot observer reader panicked".to_owned())?;
        result.and_then(encode_png)
    })
}

/// HTTP headers only prove that the watch was enqueued. `RemoteLast` runs
/// `process_remote_requests` before `process_ongoing_watching_requests`, which
/// creates the observer. Its deferred registration hook attaches `ObservedBy`
/// and installs the event runner in one exclusive World operation. A later
/// `list_components` request sees the marker only after that operation completes.
/// This entity is fresh, so no unrelated observer can supply the marker.
fn wait_for_observer(
    client: &Client,
    entity: Entity,
    receiver: &Receiver<Result<ReflectedImage, String>>,
    deadline: Instant,
) -> Result<(), String> {
    let params = to_params(&BrpListComponentsParams { entity })?;
    loop {
        check_registration_stream(receiver)?;
        let result =
            client.call_with_deadline(BRP_LIST_COMPONENTS_METHOD, Some(params.clone()), deadline);
        // Prefer the server's typed registration error to a polling timeout.
        check_registration_stream(receiver)?;
        let components: BrpListComponentsResponse = serde_json::from_value(result?)
            .map_err(|e| format!("unexpected {BRP_LIST_COMPONENTS_METHOD} result: {e}"))?;
        if components
            .iter()
            .any(|name| name == core::any::type_name::<ObservedBy>())
        {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(timeout_error("ScreenshotCaptured observer registration"));
        }
        // Unlike a fixed registration delay, polling only advances when ECS
        // confirms readiness. Waiting on the channel also surfaces errors now.
        match receiver.recv_timeout(POLL_INTERVAL.min(remaining)) {
            Ok(result) => return registration_result(result),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("BRP observer reader disconnected".to_owned());
            }
        }
    }
}

fn check_registration_stream(
    receiver: &Receiver<Result<ReflectedImage, String>>,
) -> Result<(), String> {
    match receiver.try_recv() {
        Ok(result) => registration_result(result),
        Err(TryRecvError::Empty) => Ok(()),
        Err(TryRecvError::Disconnected) => Err("BRP observer reader disconnected".to_owned()),
    }
}

fn registration_result(result: Result<ReflectedImage, String>) -> Result<(), String> {
    result
        .and_then(|_| Err("BRP received ScreenshotCaptured before inserting Screenshot".to_owned()))
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
            let _ = self.client.call_with_deadline(
                BRP_DESPAWN_COMPONENTS_METHOD,
                Some(params),
                Instant::now() + CLEANUP_BUDGET,
            );
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
    // Dimensions come from the game; checked math so hostile values can't overflow.
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&bytes| bytes > 0 && bytes <= MAX_PIXEL_BYTES && depth_or_array_layers == 1)
        .ok_or_else(|| {
            format!(
                "screenshot size {width}x{height}x{depth_or_array_layers} is empty or over the {MAX_PIXEL_BYTES} byte limit"
            )
        })?;
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
    on_result: impl FnMut(T) -> Result<Option<R>, String>,
) -> Result<R, String> {
    read_watch(
        open_watch(client, method, params, deadline)?,
        method,
        deadline,
        max_line,
        on_result,
    )
}

fn open_watch(
    client: &Client,
    method: &str,
    params: Value,
    deadline: Instant,
) -> Result<ureq::http::Response<ureq::Body>, String> {
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

    Ok(response)
}

fn read_watch<T: DeserializeOwned, R>(
    response: ureq::http::Response<ureq::Body>,
    method: &str,
    deadline: Instant,
    max_line: u64,
    mut on_result: impl FnMut(T) -> Result<Option<R>, String>,
) -> Result<R, String> {
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
        path::PathBuf,
        sync::{Condvar, Mutex},
    };

    /// What the stub BRP server sends back for one request.
    enum Reply {
        Json(Value),
        /// SSE `data:` frames, then hold the connection open for `hold`.
        Sse(Vec<Value>, Duration),
        /// Send headers now, but emit success only when insertion fires.
        SseOnInsert(Vec<Value>, Duration, Arc<(Mutex<bool>, Condvar)>),
        /// Sleep, then reply.
        Slow(Duration, Box<Reply>),
        /// A JSON-RPC error.
        Error(&'static str),
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
        let mut reply = handler(&method, &params);
        while let Reply::Slow(delay, next) = reply {
            thread::sleep(delay);
            reply = *next;
        }
        if let Reply::SseOnInsert(frames, hold, inserted) = reply {
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.flush();
            let (flag, wake) = &*inserted;
            let ready = wake
                .wait_timeout_while(flag.lock().unwrap(), Duration::from_secs(2), |flag| !*flag)
                .unwrap();
            assert!(*ready.0, "Screenshot was never inserted");
            for frame in frames {
                let _ = write!(stream, "data: {frame}\n\n");
            }
            let _ = stream.flush();
            thread::sleep(hold);
            return;
        }
        let envelope = match reply {
            Reply::Json(result) => {
                json!({ "jsonrpc": "2.0", "id": request["id"], "result": result })
            }
            Reply::Error(message) => {
                json!({ "jsonrpc": "2.0", "id": request["id"], "error": { "code": -23402, "message": message } })
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
                return;
            }
            Reply::Slow(..) | Reply::SseOnInsert(..) => unreachable!("unwrapped above"),
        };
        let body = envelope.to_string();
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }

    /// Simulates watch headers preceding observer registration. The first list
    /// sees no observer, the second sees the marker. Capture completion occurs
    /// immediately upon insertion (which also despawns the entity), never upon
    /// spawning or opening the watch.
    fn fallback_stub(
        handler: impl Fn(&str, &Value) -> Reply + Send + Sync + 'static,
    ) -> (Client, Calls) {
        let inserted = Arc::new((Mutex::new(false), Condvar::new()));
        let polls = Mutex::new(0);
        let live_entity = Mutex::new(None);
        stub(move |method, params| match method {
            BRP_SPAWN_ENTITY_METHOD => {
                assert_eq!(params["components"], json!({}));
                let reply = handler(method, params);
                if let Reply::Json(result) = &reply {
                    *live_entity.lock().unwrap() = Some(result["entity"].clone());
                }
                reply
            }
            BRP_LIST_COMPONENTS_METHOD => {
                assert_eq!(
                    live_entity.lock().unwrap().as_ref(),
                    Some(&params["entity"])
                );
                let mut polls = polls.lock().unwrap();
                *polls += 1;
                Reply::Json(if *polls == 1 {
                    json!([])
                } else {
                    json!([core::any::type_name::<ObservedBy>()])
                })
            }
            BRP_INSERT_COMPONENTS_METHOD => {
                assert!(*polls.lock().unwrap() >= 2, "observer not registered yet");
                assert_eq!(
                    params["components"][SCREENSHOT_COMPONENT]["Window"],
                    "Primary"
                );
                // A maximally fast game captures and despawns in the INSERT
                // frame. No later readiness query can rescue a missed event.
                assert_eq!(
                    live_entity.lock().unwrap().take(),
                    Some(params["entity"].clone())
                );
                let (flag, wake) = &*inserted;
                *flag.lock().unwrap() = true;
                wake.notify_one();
                Reply::Json(Value::Null)
            }
            _ => match handler(method, params) {
                Reply::Sse(frames, hold)
                    if method == BRP_OBSERVE_METHOD
                        && frames.iter().any(|frame| frame.get("result").is_some()) =>
                {
                    Reply::SseOnInsert(frames, hold, inserted.clone())
                }
                reply => reply,
            },
        })
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
        let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
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

    /// Reproduce the old-handle bug using the server's `TempPath::persist` flow.
    #[test]
    fn titan_atomic_publication_reads_the_new_inode_and_cleans_the_directory() {
        let state = Arc::new(Mutex::new((PathBuf::new(), None, 0)));
        let seen = state.clone();
        let (client, calls) = stub(move |method, params| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT, TITAN_SCREENSHOT_STATUS]),
            TITAN_SCREENSHOT => {
                let path = PathBuf::from(params["path"].as_str().unwrap());
                assert!(!path.exists(), "MCP must not precreate the destination");
                let old_handle = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .unwrap();
                // Cleanup should remove server-side staging files too.
                fs::write(
                    path.parent().unwrap().join("abandoned-staging.png"),
                    b"partial",
                )
                .unwrap();
                *seen.lock().unwrap() = (path, Some(old_handle), 0);
                Reply::Json(json!({ "token": 123 }))
            }
            TITAN_SCREENSHOT_STATUS => {
                assert_eq!(params["token"], 123);
                let mut state = seen.lock().unwrap();
                state.2 += 1;
                if state.2 == 1 {
                    return Reply::Json(json!({ "pending": true }));
                }
                let staging = tempfile::Builder::new()
                    .suffix(".png")
                    .tempfile_in(state.0.parent().unwrap())
                    .unwrap();
                fs::write(staging.path(), tiny_png()).unwrap();
                staging.into_temp_path().persist(&state.0).unwrap();
                let mut old = state.1.take().unwrap();
                let mut bytes = Vec::new();
                old.read_to_end(&mut bytes).unwrap();
                assert!(
                    bytes.is_empty(),
                    "the pre-publication handle sees the old inode"
                );
                Reply::Json(json!({ "pending": false, "path": state.0 }))
            }
            _ => panic!("unexpected {method}"),
        });
        let (info, pixels) = decode(&capture(&client, &json!({})).unwrap());
        assert_eq!((info.width, info.height), (1, 1));
        assert_eq!(pixels, [10, 20, 30]);
        assert_eq!(
            methods(&calls),
            [
                RPC_DISCOVER_METHOD,
                TITAN_SCREENSHOT,
                TITAN_SCREENSHOT_STATUS,
                TITAN_SCREENSHOT_STATUS
            ]
        );
        assert!(!state.lock().unwrap().0.parent().unwrap().exists());
    }

    #[test]
    fn fallback_decodes_reflected_bgra_image() {
        let (client, calls) = fallback_stub(|method, params| match method {
            RPC_DISCOVER_METHOD => discover(&[BRP_SPAWN_ENTITY_METHOD, BRP_OBSERVE_METHOD]),
            BRP_SPAWN_ENTITY_METHOD => {
                assert_eq!(params["components"], json!({}));
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
                BRP_OBSERVE_METHOD,
                BRP_LIST_COMPONENTS_METHOD,
                BRP_LIST_COMPONENTS_METHOD,
                BRP_INSERT_COMPONENTS_METHOD
            ]
        );
    }

    #[test]
    fn fallback_errors_and_timeouts_despawn_the_entity() {
        let (client, calls) = fallback_stub(|method, _| match method {
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
        let (client, calls) = fallback_stub(|method, _| match method {
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
    fn advertised_titan_failures_do_not_start_raw_captures() {
        for capacity_error in [false, true] {
            let (client, calls) = stub(move |method, _| match method {
                RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT]),
                TITAN_SCREENSHOT if capacity_error => Reply::Error(
                    "too many screenshots in flight or awaiting renderer cleanup (limit 64)",
                ),
                TITAN_SCREENSHOT => Reply::Json(json!({ "unexpected": true })),
                _ => panic!("must not bypass Titan with {method}"),
            });
            let err = capture(&client, &json!({})).unwrap_err();
            assert!(
                err.contains(if capacity_error {
                    "too many screenshots"
                } else {
                    "neither a `path`"
                }),
                "{err}"
            );
            assert_eq!(methods(&calls), [RPC_DISCOVER_METHOD, TITAN_SCREENSHOT]);
        }
    }

    #[test]
    fn rejects_animated_png_before_decoding_frames() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_animated(2, 0).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[1, 2, 3]).unwrap();
            writer.write_image_data(&[4, 5, 6]).unwrap();
            writer.finish().unwrap();
        }
        let error = validate_png(&bytes, Instant::now() + Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("animated PNG"), "{error}");
    }

    /// Exercise the real reflection observer hook and raw component metadata,
    /// without a GPU or window. Queuing the watch is not readiness.
    #[test]
    fn observed_by_is_a_real_registration_barrier() {
        use bevy_ecs::{
            prelude::{EntityEvent, World},
            reflect::{AppTypeRegistry, ReflectEvent},
            system::In,
        };
        use bevy_reflect::{Reflect, TypePath};
        use bevy_remote::builtin_methods::{
            process_remote_list_components_request, process_remote_observe_watching_request,
        };

        #[derive(EntityEvent, Reflect)]
        #[reflect(Event)]
        struct Captured {
            entity: Entity,
        }

        let registry = AppTypeRegistry::default();
        registry.write().register::<Captured>();
        let mut world = World::new();
        world.insert_resource(registry);
        let entity = world.spawn_empty().id();
        let list = Some(to_params(&BrpListComponentsParams { entity }).unwrap());
        let observe = Some(
            to_params(&BrpObserveParams {
                event: Captured::type_path().to_owned(),
                entity: Some(entity),
            })
            .unwrap(),
        );
        assert_eq!(
            process_remote_list_components_request(In(list.clone()), &world).unwrap(),
            json!([])
        );
        // Model the pending watch before process_ongoing_watching_requests
        // actually invokes its handler. Run the real handler on application.
        let queued = observe.clone();
        world.commands().queue(move |world: &mut World| {
            assert_eq!(
                process_remote_observe_watching_request(In(queued), world).unwrap(),
                None
            );
        });
        assert!(!world.entity(entity).contains::<ObservedBy>());
        world.flush();
        let components = process_remote_list_components_request(In(list), &world).unwrap();
        assert!(components
            .as_array()
            .unwrap()
            .contains(&json!(core::any::type_name::<ObservedBy>())));
        // ObservedBy needn't be reflected: listing uses ECS component metadata.
        assert!(world
            .resource::<AppTypeRegistry>()
            .read()
            .get(core::any::TypeId::of::<ObservedBy>())
            .is_none());
        // Simulate capture completion and despawn in the insertion frame.
        world.trigger(Captured { entity });
        world.despawn(entity);
        let captured = process_remote_observe_watching_request(In(observe), &mut world)
            .unwrap()
            .unwrap();
        assert_eq!(captured, json!([{ "entity": entity }]));
    }

    #[test]
    fn insertion_failure_cleans_up_and_joins_its_deadline_bounded_reader() {
        let (client, calls) = stub(|method, _| match method {
            RPC_DISCOVER_METHOD => discover(&[]),
            BRP_SPAWN_ENTITY_METHOD => Reply::Json(json!({ "entity": 9 })),
            BRP_OBSERVE_METHOD => Reply::Sse(vec![], Duration::from_secs(2)),
            BRP_LIST_COMPONENTS_METHOD => {
                Reply::Json(json!([core::any::type_name::<ObservedBy>()]))
            }
            BRP_INSERT_COMPONENTS_METHOD => Reply::Error("Screenshot type unavailable"),
            BRP_DESPAWN_COMPONENTS_METHOD => Reply::Json(Value::Null),
            _ => panic!("unexpected {method}"),
        });
        let start = Instant::now();
        let err = capture(&client, &json!({ "timeout_secs": 1 })).unwrap_err();
        assert!(err.contains("-23402: Screenshot type unavailable"), "{err}");
        // The stream stays open for two seconds, but its reader is joined at
        // the one-second shared deadline. No reader survives this failed call.
        assert!(start.elapsed() >= Duration::from_millis(900));
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(
            methods(&calls).last().unwrap(),
            BRP_DESPAWN_COMPONENTS_METHOD
        );
    }

    #[test]
    fn registration_timeout_never_inserts_and_cleans_up() {
        let (client, calls) = stub(|method, params| match method {
            RPC_DISCOVER_METHOD => discover(&[]),
            BRP_SPAWN_ENTITY_METHOD => {
                assert_eq!(params["components"], json!({}));
                Reply::Json(json!({ "entity": 7 }))
            }
            BRP_OBSERVE_METHOD => Reply::Sse(vec![], Duration::from_secs(2)),
            BRP_LIST_COMPONENTS_METHOD => Reply::Json(json!([])),
            BRP_DESPAWN_COMPONENTS_METHOD => Reply::Json(Value::Null),
            _ => panic!("unexpected {method}"),
        });
        let start = Instant::now();
        let err = capture(&client, &json!({ "timeout_secs": 0.2 })).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(1));
        let methods = methods(&calls);
        assert!(!methods
            .iter()
            .any(|method| method == BRP_INSERT_COMPONENTS_METHOD));
        assert_eq!(methods.last().unwrap(), BRP_DESPAWN_COMPONENTS_METHOD);
    }

    #[test]
    fn registration_error_is_not_hidden_by_polling() {
        let (client, calls) = stub(|method, _| match method {
            RPC_DISCOVER_METHOD => discover(&[]),
            BRP_SPAWN_ENTITY_METHOD => Reply::Json(json!({ "entity": 8 })),
            BRP_OBSERVE_METHOD => Reply::Slow(
                Duration::from_millis(30),
                Box::new(Reply::Sse(
                    vec![json!({ "error": { "code": -23402, "message": "Unknown event type" } })],
                    Duration::ZERO,
                )),
            ),
            // A polling call is already in flight when registration fails.
            BRP_LIST_COMPONENTS_METHOD => {
                Reply::Slow(Duration::from_millis(60), Box::new(Reply::Json(json!([]))))
            }
            BRP_DESPAWN_COMPONENTS_METHOD => Reply::Json(Value::Null),
            _ => panic!("unexpected {method}"),
        });
        let err = capture(&client, &json!({ "timeout_secs": 0.5 })).unwrap_err();
        assert!(err.contains("-23402: Unknown event type"), "{err}");
        assert!(!methods(&calls)
            .iter()
            .any(|method| method == BRP_INSERT_COMPONENTS_METHOD));
        assert_eq!(
            methods(&calls).last().unwrap(),
            BRP_DESPAWN_COMPONENTS_METHOD
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(timeout_from_args(&json!({ "timeout_secs": 0 })).is_err());
        assert!(timeout_from_args(&json!({ "timeout_secs": 61 })).is_err());
        assert!(timeout_from_args(&json!({ "timeout_secs": "5" })).is_err());
        assert_eq!(timeout_from_args(&json!({})).unwrap(), DEFAULT_TIMEOUT);

        let png = tiny_png();
        assert!(
            looks_complete(&png) && validate_png(&png, Instant::now() + DEFAULT_TIMEOUT).is_ok()
        );
        assert!(validate_png(&png, Instant::now())
            .unwrap_err()
            .contains("timed out"));
        let mut damaged = png.clone();
        let idat = damaged
            .windows(4)
            .position(|chunk| chunk == b"IDAT")
            .unwrap();
        damaged[idat + 4] ^= 1;
        assert!(looks_complete(&damaged));
        assert!(validate_png(&damaged, Instant::now() + DEFAULT_TIMEOUT).is_err());
        assert!(!looks_complete(&png[..png.len() - 1]));
        assert!(!looks_complete(b"not a png"));
        // Signature + IEND around garbage passes the cheap check but not decoding.
        let spoof = [&PNG_SIGNATURE[..], b"garbage", &PNG_IEND[..]].concat();
        assert!(
            looks_complete(&spoof)
                && validate_png(&spoof, Instant::now() + DEFAULT_TIMEOUT).is_err()
        );

        let mut buf = Vec::new();
        let mut long = BufReader::new(&b"0123456789\nok\n"[..]);
        assert!(read_line_bounded(&mut long, &mut buf, 5).is_err());
        let mut short = BufReader::new(&b"0123\nok"[..]);
        assert!(read_line_bounded(&mut short, &mut buf, 5).unwrap());
        assert_eq!(buf, b"0123\n");
        assert!(read_line_bounded(&mut short, &mut buf, 5).unwrap());
        assert!(!read_line_bounded(&mut short, &mut buf, 5).unwrap());

        let format = |format: &str, data: Vec<u8>, width: u32, height: u32, layers: u32| {
            encode_png(ReflectedImage {
                data: Some(data),
                texture_descriptor: ReflectedTextureDescriptor {
                    size: ReflectedExtent {
                        width,
                        height,
                        depth_or_array_layers: layers,
                    },
                    format: format.into(),
                },
            })
        };
        assert!(format("rgba16float", vec![0; 8], 1, 1, 1).is_err());
        assert!(format("rgba8unorm", vec![0; 4], 0, 1, 1).is_err());
        assert!(format("rgba8unorm", vec![0; 4], 1, 1, 2).is_err());
        assert!(format("rgba8unorm", vec![0; 4], 1, 1, 1).is_ok());
        // Hostile dimensions: u32::MAX² * 4 overflows u64 and must be rejected, not panic.
        for (width, height) in [(u32::MAX, 1), (u32::MAX, u32::MAX), (1 << 16, 1 << 16)] {
            let err = format("rgba8unorm", vec![0; 4], width, height, 1).unwrap_err();
            assert!(err.contains("byte limit"), "{err}");
        }
    }

    /// Every BRP call shares the user's deadline: a slow discovery or a slow
    /// instant `titan.screenshot` fails at `timeout_secs`, with no fallback
    /// started after the budget is spent.
    #[test]
    fn slow_calls_respect_timeout_secs() {
        let (client, _) = stub(|method, _| match method {
            RPC_DISCOVER_METHOD => Reply::Slow(Duration::from_secs(5), Box::new(discover(&[]))),
            _ => panic!("unexpected {method}"),
        });
        let start = Instant::now();
        assert!(capture(&client, &json!({ "timeout_secs": 0.3 })).is_err());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );

        let (client, calls) = stub(|method, _| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT]),
            TITAN_SCREENSHOT => Reply::Slow(
                Duration::from_secs(5),
                Box::new(Reply::Json(json!({ "path": "/nope" }))),
            ),
            _ => Reply::Error("unexpected"),
        });
        let start = Instant::now();
        assert!(capture(&client, &json!({ "timeout_secs": 0.3 })).is_err());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(methods(&calls), [RPC_DISCOVER_METHOD, TITAN_SCREENSHOT]);
    }

    /// A returned path other than the one requested is never opened, even if
    /// it holds a valid PNG.
    #[test]
    fn rejects_a_different_returned_path() {
        let other = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        fs::write(other.path(), tiny_png()).unwrap();
        let other_path = other.path().to_str().unwrap().to_owned();
        let (client, _) = stub(move |method, _| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT]),
            TITAN_SCREENSHOT => Reply::Json(json!({ "path": other_path })),
            _ => Reply::Error("no fallback here"),
        });
        let err = capture(&client, &json!({})).unwrap_err();
        assert!(err.contains("instead of the requested"), "{err}");
    }

    #[test]
    fn published_files_are_size_bounded_and_failures_clean_up() {
        let destination = ScreenshotDestination::new().unwrap();
        let path = PathBuf::from(&destination.path);
        let parent = path.parent().unwrap().to_owned();
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_PNG_BYTES + 1).unwrap();
        drop(file);
        let error =
            wait_for_png(&destination, Instant::now() + Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("byte limit"), "{error}");
        drop(destination);
        assert!(!parent.exists());

        let destination = ScreenshotDestination::new().unwrap();
        let parent = PathBuf::from(&destination.path)
            .parent()
            .unwrap()
            .to_owned();
        fs::create_dir(&destination.path).unwrap();
        let error =
            wait_for_png(&destination, Instant::now() + Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("securely opening"), "{error}");
        drop(destination);
        assert!(!parent.exists());
    }

    #[test]
    fn png_dimensions_cannot_bypass_the_pixel_budget() {
        let width = (MAX_PIXEL_BYTES / 4 + 1) as u32;
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, 1);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::One);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![0; (width as usize).div_ceil(8)])
                .unwrap();
        }
        assert!(looks_complete(&bytes));
        let error = validate_png(&bytes, Instant::now() + DEFAULT_TIMEOUT).unwrap_err();
        assert!(error.contains("dimensions"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_device_targets_are_never_read() {
        use std::os::unix::fs::symlink;
        let other = tempfile::NamedTempFile::new().unwrap();
        fs::write(other.path(), tiny_png()).unwrap();
        for target in [other.path(), std::path::Path::new("/dev/zero")] {
            let destination = ScreenshotDestination::new().unwrap();
            symlink(target, &destination.path).unwrap();
            let start = Instant::now();
            let error = wait_for_png(&destination, start + Duration::from_secs(1)).unwrap_err();
            assert!(error.contains("securely opening"), "{error}");
            assert!(start.elapsed() < Duration::from_secs(1));
        }
    }

    #[cfg(unix)]
    #[test]
    fn replacing_the_parent_directory_cannot_redirect_the_read() {
        use std::os::unix::fs::symlink;
        let destination = ScreenshotDestination::new().unwrap();
        let parent = PathBuf::from(&destination.path)
            .parent()
            .unwrap()
            .to_owned();
        let moved = parent.with_extension("moved");
        fs::rename(&parent, &moved).unwrap();
        let malicious = tempfile::tempdir().unwrap();
        fs::write(malicious.path().join(SCREENSHOT_FILE), tiny_png()).unwrap();
        symlink(malicious.path(), &parent).unwrap();
        // Full-path opening would accept the attacker's PNG. Anchored openat
        // still sees the original, empty directory and times out instead.
        let error =
            wait_for_png(&destination, Instant::now() + Duration::from_millis(50)).unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        drop(destination);
        assert!(malicious.path().join(SCREENSHOT_FILE).is_file());
        fs::remove_dir_all(moved).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn the_private_parent_cannot_be_replaced_on_windows() {
        use std::os::windows::fs::OpenOptionsExt;
        let destination = ScreenshotDestination::new().unwrap();
        let parent = PathBuf::from(&destination.path)
            .parent()
            .unwrap()
            .to_owned();
        assert!(fs::rename(&parent, parent.with_extension("moved")).is_err());
        assert!(fs::remove_dir(&parent).is_err());
        // A writable directory handle is needed to set a junction/reparse point.
        assert!(fs::OpenOptions::new()
            .write(true)
            .share_mode(0x00000001 | 0x00000002 | 0x00000004)
            .custom_flags(0x02000000) // FILE_FLAG_BACKUP_SEMANTICS
            .open(&parent)
            .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_symlinks_are_rejected_when_available() {
        use std::os::windows::fs::symlink_file;
        let destination = ScreenshotDestination::new().unwrap();
        let other = tempfile::NamedTempFile::new().unwrap();
        fs::write(other.path(), tiny_png()).unwrap();
        // Windows may not grant symlink creation without developer mode.
        match symlink_file(other.path(), &destination.path) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(1314) => return,
            Err(error) => panic!("symlink creation failed: {error}"),
        }
        let error =
            wait_for_png(&destination, Instant::now() + Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("reparse point"), "{error}");
    }

    /// Paths the game controls can be FIFOs. Neither returning one nor
    /// swapping one in at the requested path may block the read.
    #[cfg(unix)]
    #[test]
    fn fifo_paths_do_not_block() {
        let mkfifo = |path: &str| {
            let status = std::process::Command::new("mkfifo")
                .arg(path)
                .status()
                .unwrap();
            assert!(status.success());
        };
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("evil.png").to_str().unwrap().to_owned();
        mkfifo(&fifo);
        let (client, _) = stub(move |method, _| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT]),
            TITAN_SCREENSHOT => Reply::Json(json!({ "path": fifo })),
            _ => Reply::Error("no fallback here"),
        });
        let err = capture(&client, &json!({ "timeout_secs": 2 })).unwrap_err();
        assert!(err.contains("instead of the requested"), "{err}");

        let (client, _) = stub(move |method, params| match method {
            RPC_DISCOVER_METHOD => discover(&[TITAN_SCREENSHOT]),
            TITAN_SCREENSHOT => {
                let path = params["path"].as_str().unwrap();
                mkfifo(path);
                Reply::Json(json!({ "path": path }))
            }
            _ => Reply::Error("no fallback here"),
        });
        let start = Instant::now();
        let err = capture(&client, &json!({ "timeout_secs": 0.5 })).unwrap_err();
        assert!(err.contains("not a regular file"), "{err}");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
    }
}
