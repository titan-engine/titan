//! `titan.screenshot` and `titan.screenshot_status`: save the primary window to a PNG file.
//!
//! Screenshots are captured asynchronously, a few frames after they are requested, so the
//! methods use a token design instead of blocking the request:
//!
//! 1. `titan.screenshot` with params `{ "path"?: string }` queues a capture and returns
//!    `{ "token": u64 }` immediately.
//! 2. `titan.screenshot_status` with params `{ "token": u64 }` returns `{ "pending": true }` while
//!    the capture is in flight, and `{ "pending": false, "path": string }` once the PNG exists on
//!    disk. If the capture failed or timed out, it returns an error instead.
//!
//! # Paths
//!
//! `path` must end in `.png` (any case). Relative paths are resolved against the game's current
//! working directory, and the parent directory must already exist. The result always holds the
//! absolute path. Without `path`, the file goes to a per-process temporary directory as
//! `titan-screenshot-<token>.png`.
//!
//! # Guarantees
//!
//! - **Only real writes complete.** The image is first written to a hidden temporary file next to
//!   the destination (using [`save_to_disk`]), then renamed over the destination. A job only
//!   reports `pending: false` after that rename succeeds, so a file that already existed at the
//!   destination can never be mistaken for the new screenshot, and a failed write leaves it
//!   untouched.
//! - **Concurrent requests are serialized.** The renderer drops duplicate screenshots of the same
//!   window within a frame, so jobs are queued and captured one at a time, in request order.
//! - **Storage is bounded.** At most [`MAX_JOBS`] jobs are tracked. Finished jobs are kept for
//!   [`RETENTION`] so clients can poll them, and are evicted early (oldest first) when space is
//!   needed. A job that hasn't finished [`TIMEOUT`] after it was requested fails. Timeouts use
//!   wall-clock time, so they still work while `Time<Virtual>` is paused.
//!
//! Tokens are only meaningful within one run of the game. Expired or unknown tokens return an
//! `INVALID_PARAMS` error.
//!
//! The PNG is encoded on the main thread inside [`save_to_disk`]'s observer, as upstream does.
//! Nothing else here blocks on image work.

use alloc::{collections::VecDeque, format, string::String};
use core::time::Duration;
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Instant,
};

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use bevy_remote::{error_codes, BrpError, BrpResult, RemoteMethodSystemId, RemoteMethods};
use bevy_render::{
    view::screenshot::{save_to_disk, Capturing, Screenshot, ScreenshotCaptured},
    RenderApp,
};
use bevy_window::PrimaryWindow;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The method name for queueing a screenshot.
pub const SCREENSHOT_METHOD: &str = "titan.screenshot";

/// The method name for polling a screenshot queued with [`SCREENSHOT_METHOD`].
pub const SCREENSHOT_STATUS_METHOD: &str = "titan.screenshot_status";

/// The most screenshot jobs tracked at once, finished or not.
pub const MAX_JOBS: usize = 64;

/// How long a job may take, from request to file on disk, before it fails.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// How long a finished job stays available to `titan.screenshot_status`.
pub const RETENTION: Duration = Duration::from_secs(300);

/// Registers the screenshot methods and the systems that drive them.
///
/// Must be called after `RemotePlugin` has been added, so that [`RemoteMethods`] exists.
pub(super) fn register(app: &mut App) {
    let renderer_available = app.get_sub_app(RenderApp).is_some();
    app.insert_resource(ScreenshotJobs::new(renderer_available))
        .add_systems(Update, drive_screenshot_jobs);

    let screenshot = app.register_system(process_screenshot_request);
    let status = app.register_system(process_screenshot_status_request);
    let mut methods = app.world_mut().resource_mut::<RemoteMethods>();
    methods.insert(SCREENSHOT_METHOD, RemoteMethodSystemId::Instant(screenshot));
    methods.insert(
        SCREENSHOT_STATUS_METHOD,
        RemoteMethodSystemId::Instant(status),
    );
}

/// Params for `titan.screenshot`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ScreenshotParams {
    path: Option<PathBuf>,
}

/// Result of `titan.screenshot`.
#[derive(Debug, Serialize)]
struct ScreenshotResponse {
    token: u64,
}

/// Params for `titan.screenshot_status`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenshotStatusParams {
    token: u64,
}

/// Result of `titan.screenshot_status`.
#[derive(Debug, Serialize)]
struct ScreenshotStatusResponse {
    pending: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
}

#[derive(Debug)]
enum JobState {
    /// Waiting for an earlier capture to finish.
    Queued,
    /// A [`Screenshot`] entity has been spawned for this job.
    Capturing(Entity),
    /// The PNG is at the job's destination.
    Done,
    /// The capture or write failed.
    Failed(String),
}

#[derive(Debug)]
struct Job {
    token: u64,
    destination: PathBuf,
    requested: Instant,
    /// When the job reached [`JobState::Done`] or [`JobState::Failed`].
    finished: Option<Instant>,
    state: JobState,
}

impl Job {
    fn finish(&mut self, state: JobState) {
        self.state = state;
        self.finished = Some(Instant::now());
    }
}

/// Tracks screenshot jobs requested over BRP.
#[derive(Resource, Debug)]
struct ScreenshotJobs {
    /// Jobs in request order.
    jobs: VecDeque<Job>,
    next_token: u64,
    renderer_available: bool,
    timeout: Duration,
    retention: Duration,
    /// Lazily created directory for screenshots requested without a path.
    default_dir: Option<PathBuf>,
}

impl ScreenshotJobs {
    fn new(renderer_available: bool) -> Self {
        Self {
            jobs: VecDeque::new(),
            next_token: 1,
            renderer_available,
            timeout: TIMEOUT,
            retention: RETENTION,
            default_dir: None,
        }
    }

    fn get_mut(&mut self, token: u64) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.token == token)
    }

    /// Drops finished jobs past their retention period, then, if still full, the oldest finished
    /// job. Returns `false` if every slot holds an unfinished job.
    fn make_room(&mut self) -> bool {
        let retention = self.retention;
        self.jobs
            .retain(|job| job.finished.is_none_or(|at| at.elapsed() < retention));
        if self.jobs.len() < MAX_JOBS {
            return true;
        }
        let oldest_finished = self
            .jobs
            .iter()
            .enumerate()
            .filter_map(|(index, job)| job.finished.map(|at| (index, at)))
            .min_by_key(|&(_, at)| at)
            .map(|(index, _)| index);
        match oldest_finished {
            Some(index) => {
                self.jobs.remove(index);
                true
            }
            None => false,
        }
    }

    fn default_dir(&mut self) -> io::Result<PathBuf> {
        if let Some(dir) = &self.default_dir {
            return Ok(dir.clone());
        }
        let dir = tempfile::Builder::new()
            .prefix("titan-screenshots-")
            .tempdir()?
            .keep();
        self.default_dir = Some(dir.clone());
        Ok(dir)
    }
}

/// Handles `titan.screenshot`.
fn process_screenshot_request(
    In(params): In<Option<Value>>,
    mut jobs: ResMut<ScreenshotJobs>,
    primary_window: Query<(), With<PrimaryWindow>>,
) -> BrpResult {
    let ScreenshotParams { path } = match params {
        Some(params) => parse(params)?,
        None => ScreenshotParams::default(),
    };

    if !jobs.renderer_available {
        return Err(internal_error(
            "screenshots need a renderer, but this app has no render sub-app",
        ));
    }
    if primary_window.is_empty() {
        return Err(internal_error(
            "screenshots need a primary window, but this app has none",
        ));
    }

    let token = jobs.next_token;
    let destination = match path {
        Some(path) => validate_destination(&path)?,
        None => jobs
            .default_dir()
            .map_err(|e| internal_error(format!("cannot create temporary directory: {e}")))?
            .join(format!("titan-screenshot-{token}.png")),
    };

    if !jobs.make_room() {
        return Err(internal_error(format!(
            "too many screenshots in flight (limit {MAX_JOBS}); poll existing tokens first"
        )));
    }

    jobs.next_token += 1;
    jobs.jobs.push_back(Job {
        token,
        destination,
        requested: Instant::now(),
        finished: None,
        state: JobState::Queued,
    });

    serde_json::to_value(ScreenshotResponse { token }).map_err(BrpError::internal)
}

/// Handles `titan.screenshot_status`.
fn process_screenshot_status_request(
    In(params): In<Option<Value>>,
    mut jobs: ResMut<ScreenshotJobs>,
) -> BrpResult {
    let Some(params) = params else {
        return Err(invalid_params("params `{ \"token\": u64 }` are required"));
    };
    let ScreenshotStatusParams { token } = parse(params)?;

    let Some(job) = jobs.get_mut(token) else {
        return Err(invalid_params(format!(
            "unknown or expired screenshot token {token}"
        )));
    };
    let response = match &job.state {
        JobState::Queued | JobState::Capturing(_) => ScreenshotStatusResponse {
            pending: true,
            path: None,
        },
        JobState::Done => ScreenshotStatusResponse {
            pending: false,
            path: Some(job.destination.clone()),
        },
        JobState::Failed(reason) => {
            return Err(internal_error(format!(
                "screenshot {token} failed: {reason}"
            )));
        }
    };
    serde_json::to_value(response).map_err(BrpError::internal)
}

/// Times out stale jobs and starts the next queued capture when none is in flight.
fn drive_screenshot_jobs(
    mut commands: Commands,
    mut jobs: ResMut<ScreenshotJobs>,
    capturing: Query<Has<Capturing>>,
) {
    let retention = jobs.retention;
    jobs.jobs
        .retain(|job| job.finished.is_none_or(|at| at.elapsed() < retention));
    let timeout = jobs.timeout;
    let mut in_flight = false;
    for job in jobs.jobs.iter_mut() {
        let JobState::Capturing(entity) = job.state else {
            continue;
        };
        match capturing.get(entity) {
            Err(_) => job.finish(JobState::Failed(
                "the screenshot was dropped before it was captured, possibly because \
                 another screenshot of the primary window was taken in the same frame"
                    .into(),
            )),
            Ok(started) if job.requested.elapsed() >= timeout => {
                // Once the renderer has picked the screenshot up, despawning it could race with
                // upstream's own cleanup. The observer ignores late captures instead.
                if !started {
                    commands.entity(entity).despawn();
                }
                job.finish(JobState::Failed(format!("timed out after {timeout:?}")));
            }
            Ok(_) => in_flight = true,
        }
    }

    for job in jobs.jobs.iter_mut() {
        if matches!(job.state, JobState::Queued) && job.requested.elapsed() >= timeout {
            job.finish(JobState::Failed(format!("timed out after {timeout:?}")));
        }
    }

    if in_flight {
        return;
    }
    let Some(job) = jobs
        .jobs
        .iter_mut()
        .find(|job| matches!(job.state, JobState::Queued))
    else {
        return;
    };

    let token = job.token;
    let destination = job.destination.clone();
    // Reserve an unpredictable, private staging file. Never reuse a pathname from
    // a previous run, or follow a pre-existing symlink when the encoder opens it.
    let partial = match tempfile::Builder::new()
        .prefix(".titan-screenshot-")
        .suffix(".png")
        .tempfile_in(
            destination
                .parent()
                .expect("validated destination has a parent"),
        ) {
        Ok(file) => file.into_temp_path(),
        Err(error) => {
            job.finish(JobState::Failed(format!(
                "cannot create staging file: {error}"
            )));
            return;
        }
    };
    let mut save = save_to_disk(partial.to_path_buf());
    let mut partial = Some(partial);
    let entity = commands
        .spawn(Screenshot::primary_window())
        .observe(
            move |captured: On<ScreenshotCaptured>, mut jobs: ResMut<ScreenshotJobs>| {
                let Some(job) = jobs.get_mut(token) else {
                    return;
                };
                if !matches!(job.state, JobState::Capturing(_)) {
                    // Timed out or otherwise abandoned.
                    return;
                }
                let Some(partial) = partial.take() else {
                    return;
                };
                save(captured);
                // save_to_disk logs encoding/IO errors without returning a Result.
                // File existence is insufficient: a failed write can leave a
                // zero-length or truncated file. PNG's final IEND chunk is written
                // only after its image data; require it before publishing the file.
                let result = complete_png(&partial)
                    .map_err(|e| {
                        format!("the PNG was not completely written ({e}); see the game's log")
                    })
                    .and_then(|()| {
                        partial.persist(&destination).map_err(|e| {
                            format!("cannot move the PNG to {}: {e}", destination.display())
                        })
                    });
                match result {
                    Ok(()) => job.finish(JobState::Done),
                    Err(reason) => job.finish(JobState::Failed(reason)),
                }
            },
        )
        .id();
    job.state = JobState::Capturing(entity);
}

/// Checks a requested destination and makes it absolute.
fn validate_destination(path: &Path) -> Result<PathBuf, BrpError> {
    let is_png = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
    if !is_png {
        return Err(invalid_params(format!(
            "screenshot path must end in `.png`: {}",
            path.display()
        )));
    }
    let path = std::path::absolute(path)
        .map_err(|e| invalid_params(format!("invalid path {}: {e}", path.display())))?;
    if path.is_dir() {
        return Err(invalid_params(format!(
            "screenshot path is a directory: {}",
            path.display()
        )));
    }
    match path.parent() {
        Some(parent) if parent.is_dir() => Ok(path),
        _ => Err(invalid_params(format!(
            "the parent directory of {} does not exist",
            path.display()
        ))),
    }
}

/// Checks that our freshly encoded PNG has its signature and final IEND chunk.
///
/// This is a completion check for the trusted PNG encoder, not a general-purpose
/// PNG validator. It reads only 20 bytes instead of decoding the image a second time.
fn complete_png(path: &Path) -> io::Result<()> {
    let mut file = fs::File::open(path)?;
    let mut signature = [0; 8];
    file.read_exact(&mut signature)?;
    if signature != *b"\x89PNG\r\n\x1a\n" {
        return Err(io::Error::other("missing PNG signature"));
    }
    file.seek(SeekFrom::End(-12))?;
    let mut trailer = [0; 12];
    file.read_exact(&mut trailer)?;
    if trailer != *b"\x00\x00\x00\x00IEND\xaeB\x60\x82" {
        return Err(io::Error::other("missing PNG end marker"));
    }
    Ok(())
}

fn parse<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, BrpError> {
    serde_json::from_value(params).map_err(|e| invalid_params(e.to_string()))
}

fn invalid_params(message: impl Into<String>) -> BrpError {
    BrpError {
        code: error_codes::INVALID_PARAMS,
        message: message.into(),
        data: None,
    }
}

fn internal_error(message: impl Into<String>) -> BrpError {
    BrpError {
        code: error_codes::INTERNAL_ERROR,
        message: message.into(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_image::Image;
    use bevy_window::Window;
    use serde_json::json;

    /// An app with the screenshot methods, a primary window, and a pretend renderer.
    ///
    /// Nothing is ever rendered: tests trigger [`ScreenshotCaptured`] themselves.
    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<RemoteMethods>();
        register(&mut app);
        app.world_mut()
            .resource_mut::<ScreenshotJobs>()
            .renderer_available = true;
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app
    }

    fn call(app: &mut App, method: &str, params: Option<Value>) -> BrpResult {
        let Some(RemoteMethodSystemId::Instant(id)) =
            app.world().resource::<RemoteMethods>().get(method).cloned()
        else {
            panic!("{method} is not registered as an instant method");
        };
        app.world_mut().run_system_with(id, params).unwrap()
    }

    fn request(app: &mut App, path: &Path) -> u64 {
        let result = call(app, SCREENSHOT_METHOD, Some(json!({ "path": path }))).unwrap();
        result["token"].as_u64().unwrap()
    }

    fn status(app: &mut App, token: u64) -> BrpResult {
        call(
            app,
            SCREENSHOT_STATUS_METHOD,
            Some(json!({ "token": token })),
        )
    }

    /// The single [`Screenshot`] entity currently waiting for capture.
    fn screenshot_entity(app: &mut App) -> Entity {
        let mut query = app.world_mut().query_filtered::<Entity, With<Screenshot>>();
        query.single(app.world()).unwrap()
    }

    fn capture(app: &mut App, image: Image) {
        let entity = screenshot_entity(app);
        app.world_mut()
            .trigger(ScreenshotCaptured { entity, image });
    }

    #[test]
    fn completes_only_after_the_file_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        fs::write(&path, b"stale").unwrap();
        let mut app = app();

        let token = request(&mut app, &path);
        assert_eq!(status(&mut app, token).unwrap(), json!({ "pending": true }));
        app.update();
        assert_eq!(status(&mut app, token).unwrap(), json!({ "pending": true }));

        capture(&mut app, Image::default());
        assert_eq!(
            status(&mut app, token).unwrap(),
            json!({ "pending": false, "path": path })
        );
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
        // Only the destination is left behind.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn defaults_to_a_temporary_png() {
        let mut app = app();
        let token = call(&mut app, SCREENSHOT_METHOD, None).unwrap()["token"]
            .as_u64()
            .unwrap();
        app.update();
        capture(&mut app, Image::default());

        let result = status(&mut app, token).unwrap();
        let path = PathBuf::from(result["path"].as_str().unwrap());
        assert!(path.is_absolute());
        assert_eq!(path.extension().unwrap(), "png");
        assert!(path.is_file());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn failed_write_does_not_complete_or_touch_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        fs::write(&path, b"stale").unwrap();
        let mut app = app();

        let token = request(&mut app, &path);
        app.update();
        // An image without data can't be encoded, so `save_to_disk` writes nothing.
        capture(
            &mut app,
            Image {
                data: None,
                ..Image::default()
            },
        );

        let error = status(&mut app, token).unwrap_err();
        assert_eq!(error.code, error_codes::INTERNAL_ERROR);
        assert_eq!(fs::read(&path).unwrap(), b"stale");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_requests_are_captured_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let first_path = dir.path().join("first.png");
        let second_path = dir.path().join("second.png");
        let mut app = app();

        let first = request(&mut app, &first_path);
        let second = request(&mut app, &second_path);
        assert_ne!(first, second);

        app.update();
        // `screenshot_entity` asserts there is exactly one.
        capture(&mut app, Image::default());
        assert_eq!(status(&mut app, first).unwrap()["pending"], false);
        assert_eq!(status(&mut app, second).unwrap()["pending"], true);

        // Upstream despawns captured screenshots in `First`; do the same by hand.
        let entity = screenshot_entity(&mut app);
        app.world_mut().despawn(entity);
        app.update();
        capture(&mut app, Image::default());
        assert_eq!(
            status(&mut app, second).unwrap(),
            json!({ "pending": false, "path": second_path })
        );
    }

    #[test]
    fn times_out_and_ignores_late_captures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        let mut app = app();

        let token = request(&mut app, &path);
        app.update();
        // Pretend the renderer picked it up, so the entity survives the timeout.
        let entity = screenshot_entity(&mut app);
        app.world_mut().entity_mut(entity).insert(Capturing);
        app.world_mut().resource_mut::<ScreenshotJobs>().timeout = Duration::ZERO;
        app.update();
        assert!(status(&mut app, token).is_err());

        capture(&mut app, Image::default());
        assert!(status(&mut app, token).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn dropped_screenshot_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        let token = request(&mut app, &dir.path().join("shot.png"));
        app.update();
        let entity = screenshot_entity(&mut app);
        app.world_mut().despawn(entity);
        app.update();
        assert!(status(&mut app, token).is_err());
    }

    #[test]
    fn rejects_bad_requests() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();

        for params in [
            json!({ "path": dir.path().join("shot.jpg") }),
            json!({ "path": dir.path().join("shot") }),
            json!({ "path": dir.path().join("missing/shot.png") }),
            json!({ "path": 3 }),
            json!({ "unknown": true }),
        ] {
            let error = call(&mut app, SCREENSHOT_METHOD, Some(params.clone())).unwrap_err();
            assert_eq!(error.code, error_codes::INVALID_PARAMS, "{params}");
        }

        let error = status(&mut app, 999).unwrap_err();
        assert_eq!(error.code, error_codes::INVALID_PARAMS);
        let error = call(&mut app, SCREENSHOT_STATUS_METHOD, None).unwrap_err();
        assert_eq!(error.code, error_codes::INVALID_PARAMS);
    }

    #[test]
    fn needs_a_renderer_and_a_primary_window() {
        let mut no_renderer = app();
        no_renderer
            .world_mut()
            .resource_mut::<ScreenshotJobs>()
            .renderer_available = false;
        assert!(call(&mut no_renderer, SCREENSHOT_METHOD, None).is_err());

        let mut no_window = app();
        let mut windows = no_window
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        let window = windows.single(no_window.world()).unwrap();
        no_window.world_mut().despawn(window);
        assert!(call(&mut no_window, SCREENSHOT_METHOD, None).is_err());
    }

    #[test]
    fn incomplete_pngs_are_not_published() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.png");
        for bytes in [
            &b""[..],
            &b"stale"[..],
            &b"\x89PNG\r\n\x1a\ntruncated-data"[..],
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(complete_png(&path).is_err());
        }
    }

    #[test]
    fn finished_tokens_expire_without_another_request() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        let token = request(&mut app, &dir.path().join("shot.png"));
        app.update();
        capture(&mut app, Image::default());
        assert_eq!(status(&mut app, token).unwrap()["pending"], false);
        app.world_mut().resource_mut::<ScreenshotJobs>().retention = Duration::ZERO;
        app.update();
        assert_eq!(
            status(&mut app, token).unwrap_err().code,
            error_codes::INVALID_PARAMS
        );
        assert!(dir.path().join("shot.png").exists());
    }

    #[test]
    fn job_storage_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        let mut app = app();

        for _ in 0..MAX_JOBS {
            request(&mut app, &path);
        }
        assert!(call(&mut app, SCREENSHOT_METHOD, Some(json!({ "path": path }))).is_err());

        // Finishing a job frees a slot, evicting the oldest finished job when needed.
        app.update();
        capture(&mut app, Image::default());
        let token = request(&mut app, &path);
        assert_eq!(status(&mut app, token).unwrap()["pending"], true);
        assert!(status(&mut app, 1).is_err());
    }
}
