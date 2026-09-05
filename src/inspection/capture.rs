//! Owned capture completion, independent of application and simulation lifetimes.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::Duration;
use titan_protocol::{
    CaptureIdentity, CaptureResult, ErrorCode, ProtocolError, RequestEnvelope, Response,
    ResponseEnvelope,
};

/// Host-enforced admission and end-to-end deadline limits.
#[derive(Clone, Copy, Debug)]
pub struct CaptureLimits {
    pub max_outstanding: usize,
    pub max_dimension: u32,
    pub max_rgba_bytes: u64,
    pub max_artifact_bytes: usize,
    pub timeout: Duration,
}
impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            max_outstanding: 1,
            max_dimension: 2048,
            max_rgba_bytes: 16 * 1024 * 1024,
            max_artifact_bytes: 3 * 1024 * 1024,
            timeout: Duration::from_secs(5),
        }
    }
}
impl CaptureLimits {
    pub fn validate_dimensions(&self, width: u32, height: u32) -> Result<(), ProtocolError> {
        if width == 0
            || height == 0
            || width > self.max_dimension
            || height > self.max_dimension
            || (u64::from(width) * u64::from(height))
                .checked_mul(4)
                .is_none_or(|bytes| bytes > self.max_rgba_bytes)
        {
            Err(ProtocolError::new(
                ErrorCode::InvalidValue,
                "capture dimensions exceed configured limits",
            ))
        } else {
            Ok(())
        }
    }
}

pub enum Dispatch {
    Ready(ResponseEnvelope),
    Pending(PendingCapture),
}
impl Dispatch {
    /// Compatibility for synchronous consumers. Async hosts must retain pending work.
    pub fn into_ready(self) -> ResponseEnvelope {
        match self {
            Self::Ready(response) => response,
            Self::Pending(mut pending) => pending
                .finish_error(
                    ErrorCode::Unsupported,
                    "asynchronous capture requires dispatch completion support",
                )
                .unwrap(),
        }
    }
}
struct State {
    result: Option<Result<CaptureResult, ProtocolError>>,
    abandoned: bool,
    producer_done: bool,
    outstanding: Arc<AtomicUsize>,
}
impl Drop for State {
    fn drop(&mut self) {
        self.outstanding.fetch_sub(1, Ordering::AcqRel);
    }
}
/// Owned producer. Keep this alive until submitted work and its resources are reclaimed.
/// Dropping an unfinished producer publishes a bounded failure. Late results are discarded.
pub struct CaptureCompleter {
    state: Arc<Mutex<State>>,
    limits: CaptureLimits,
}
impl CaptureCompleter {
    pub fn complete(self, result: Result<CaptureResult, ProtocolError>) {
        let result = result.and_then(|result| {
            if result.artifact.len() > self.limits.max_artifact_bytes
                || result.format.len() > 64
                || result.checksum.len() > 128
            {
                Err(ProtocolError::new(
                    ErrorCode::InvalidValue,
                    "capture output exceeds retention limits",
                ))
            } else {
                Ok(result)
            }
        });
        let mut state = self.state.lock().unwrap();
        if !state.abandoned {
            state.result = Some(result);
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.state.lock().unwrap().abandoned
    }
}
impl Drop for CaptureCompleter {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.producer_done = true;
    }
}
/// A single eventual response. Poll with host monotonic time elapsed since acceptance,
/// including extraction, rendering, readback and encoding. No application borrow is held.
pub struct PendingCapture {
    request: RequestEnvelope,
    identity: CaptureIdentity,
    state: Arc<Mutex<State>>,
    generation: Arc<AtomicU64>,
    limits: CaptureLimits,
    finished: bool,
    #[cfg(not(target_arch = "wasm32"))]
    started: std::time::Instant,
}
impl PendingCapture {
    pub(super) fn new(
        request: RequestEnvelope,
        identity: CaptureIdentity,
        generation: Arc<AtomicU64>,
        outstanding: Arc<AtomicUsize>,
        limits: CaptureLimits,
    ) -> (Self, CaptureCompleter) {
        outstanding.fetch_add(1, Ordering::AcqRel);
        let state = Arc::new(Mutex::new(State {
            result: None,
            abandoned: false,
            producer_done: false,
            outstanding,
        }));
        (
            Self {
                request,
                identity,
                state: state.clone(),
                generation,
                limits,
                finished: false,
                #[cfg(not(target_arch = "wasm32"))]
                started: std::time::Instant::now(),
            },
            CaptureCompleter { state, limits },
        )
    }
    pub fn identity(&self) -> &CaptureIdentity {
        &self.identity
    }
    pub fn cancel(&mut self) -> Option<ResponseEnvelope> {
        self.finish_error(ErrorCode::Cancelled, "capture cancelled")
    }
    fn finish_error(&mut self, code: ErrorCode, message: &str) -> Option<ResponseEnvelope> {
        if self.finished {
            return None;
        }
        self.finished = true;
        let mut state = self.state.lock().unwrap();
        state.abandoned = true;
        state.result = None;
        Some(self.response(Err(ProtocolError::new(code, message))))
    }
    fn response(&self, result: Result<CaptureResult, ProtocolError>) -> ResponseEnvelope {
        let result = result.map_err(|mut error| {
            error.message = error
                .message
                .chars()
                .filter(|ch| !ch.is_control())
                .take(512)
                .collect();
            if serde_json::to_vec(&error.details).map_or(true, |bytes| bytes.len() > 2048) {
                error.details.clear();
            }
            error
        });
        let id = &self.identity;
        match result {
            Ok(result) => ResponseEnvelope::success(
                &self.request,
                &id.instance_id,
                id.observed_frame,
                id.state_revision,
                Response::Capture(result),
            ),
            Err(error) => ResponseEnvelope::failure(
                &self.request,
                &id.instance_id,
                id.observed_frame,
                id.state_revision,
                error,
            ),
        }
    }
    pub fn poll(&mut self, elapsed: Duration) -> Option<ResponseEnvelope> {
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed = elapsed.max(self.started.elapsed());
        if self.finished {
            return None;
        }
        if self.generation.load(Ordering::Acquire) != self.identity.session_generation {
            return self.finish_error(ErrorCode::Cancelled, "capture session was reset");
        }
        if elapsed >= self.limits.timeout {
            return self.finish_error(ErrorCode::Timeout, "capture deadline exceeded");
        }
        let result = {
            let mut state = self.state.lock().unwrap();
            match state.result.take() {
                Some(result) => Some(result),
                None if state.producer_done => Some(Err(ProtocolError::new(
                    ErrorCode::Internal,
                    "capture producer ended without a result",
                ))),
                None => None,
            }
        }?;
        let result = result.and_then(|mut result| {
            if result.width != self.identity.width
                || result.height != self.identity.height
                || result.artifact.len() > self.limits.max_artifact_bytes
                || result.format.len() > 64
                || result.checksum.len() > 128
            {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidValue,
                    "capture output exceeds accepted dimensions or artifact limits",
                ));
            }
            result.identity = self.identity.clone();
            let response = self.response(Ok(result.clone()));
            if serde_json::to_vec(&response).map_or(true, |bytes| bytes.len() > 4 * 1024 * 1024) {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidValue,
                    "capture response exceeds transport envelope limit",
                ));
            }
            Ok(result)
        });
        let response = self.response(result);
        #[cfg(not(target_arch = "wasm32"))]
        if self.started.elapsed() >= self.limits.timeout {
            return self.finish_error(
                ErrorCode::Timeout,
                "capture deadline exceeded during response preparation",
            );
        }
        self.finished = true;
        Some(response)
    }
}
impl Drop for PendingCapture {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.abandoned = true;
        state.result = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        App,
        inspection::{InspectionConfig, Inspector},
    };
    use titan_protocol::{Request, ResponseOutcome};
    fn result(identity: &CaptureIdentity, artifact: &str) -> CaptureResult {
        CaptureResult {
            identity: identity.clone(),
            width: identity.width,
            height: identity.height,
            format: "fixture".into(),
            artifact: artifact.into(),
            checksum: "fixture".into(),
        }
    }
    fn capture(inspector: &mut Inspector, app: &mut App, id: &str) -> PendingCapture {
        match inspector.dispatch(app, &RequestEnvelope::new(id, Request::Capture)) {
            Dispatch::Pending(pending) => pending,
            _ => panic!("expected pending"),
        }
    }
    fn failure(response: ResponseEnvelope, code: ErrorCode) {
        assert!(matches!(response.outcome,ResponseOutcome::Failure { error } if error.code==code));
    }
    #[test]
    fn owned_snapshot_reports_acceptance_after_live_changes_and_resize() {
        let mut inspector = Inspector::new(InspectionConfig::controlled("runtime", "test"));
        let mut app = App::new();
        app.world_mut().insert_resource(7_u32);
        let work = Arc::new(Mutex::new(None));
        let handler_work = work.clone();
        inspector.register_async_capture_handler(8, 4, move |app, identity, completion| {
            *handler_work.lock().unwrap() = Some((
                identity,
                *app.world().resource::<u32>().unwrap(),
                completion,
            ));
            Ok(())
        });
        let mut pending = capture(&mut inspector, &mut app, "a");
        app.world_mut().insert_resource(9_u32);
        inspector.note_external_change();
        inspector.set_capture_dimensions(2, 2);
        inspector.handle(
            &mut app,
            &RequestEnvelope::new("step", Request::Step { frames: 3 }),
        );
        let (identity, snapshot, completion) = work.lock().unwrap().take().unwrap();
        assert_eq!(snapshot, 7);
        completion.complete(Ok(result(&identity, "owned-7")));
        let response = pending.poll(Duration::ZERO).unwrap();
        assert_eq!(response.observed_frame, 0);
        assert_eq!(response.state_revision, 0);
        assert_eq!(response.request_id, "a");
        let ResponseOutcome::Success {
            response: Response::Capture(captured),
        } = response.outcome
        else {
            panic!("capture")
        };
        assert_eq!(captured.identity, identity);
        assert_eq!((captured.width, captured.height), (8, 4));
        assert!(pending.poll(Duration::ZERO).is_none());
    }
    #[test]
    fn cancellation_timeout_reset_hold_admission_until_late_resources_reclaimed() {
        for mode in 0..3 {
            let mut inspector = Inspector::new(InspectionConfig::controlled("runtime", "test"));
            let mut app = App::new();
            let work = Arc::new(Mutex::new(None));
            let handler_work = work.clone();
            inspector.register_async_capture_handler(2, 2, move |_, identity, completion| {
                *handler_work.lock().unwrap() = Some((identity, completion));
                Ok(())
            });
            let mut pending = capture(&mut inspector, &mut app, "a");
            let response = match mode {
                0 => pending.cancel(),
                1 => pending.poll(Duration::from_secs(5)),
                _ => {
                    inspector.reset_capture_session();
                    pending.poll(Duration::ZERO)
                }
            }
            .unwrap();
            failure(
                response,
                if mode == 1 {
                    ErrorCode::Timeout
                } else {
                    ErrorCode::Cancelled
                },
            );
            assert!(pending.poll(Duration::ZERO).is_none());
            drop(pending);
            failure(
                inspector
                    .dispatch(&mut app, &RequestEnvelope::new("busy", Request::Capture))
                    .into_ready(),
                ErrorCode::Busy,
            );
            let (identity, completion) = work.lock().unwrap().take().unwrap();
            assert!(completion.is_cancelled());
            completion.complete(Ok(result(&identity, "late")));
            let fresh = capture(&mut inspector, &mut app, "b");
            assert_eq!(fresh.identity().capture_id, 2);
            assert_eq!(fresh.identity().session_generation, u64::from(mode == 2));
        }
    }
    #[test]
    fn dimensions_output_and_failure_are_bounded_and_completed_results_hold_slot() {
        let mut inspector = Inspector::new(InspectionConfig::controlled("runtime", "test"));
        let mut app = App::new();
        let work = Arc::new(Mutex::new(None));
        let handler_work = work.clone();
        inspector.register_async_capture_handler(0, 1, move |_, id, completion| {
            *handler_work.lock().unwrap() = Some((id, completion));
            Ok(())
        });
        failure(
            inspector
                .dispatch(&mut app, &RequestEnvelope::new("invalid", Request::Capture))
                .into_ready(),
            ErrorCode::InvalidValue,
        );
        assert!(work.lock().unwrap().is_none());
        inspector.set_capture_dimensions(1, 1);
        let mut pending = capture(&mut inspector, &mut app, "valid");
        let (identity, completion) = work.lock().unwrap().take().unwrap();
        let mut output = result(&identity, "image");
        output.width = 2;
        completion.complete(Ok(output));
        failure(
            inspector
                .dispatch(&mut app, &RequestEnvelope::new("busy", Request::Capture))
                .into_ready(),
            ErrorCode::Busy,
        );
        failure(
            pending.poll(Duration::ZERO).unwrap(),
            ErrorCode::InvalidValue,
        );
        drop(pending);
        let mut failed = capture(&mut inspector, &mut app, "failed");
        drop(work.lock().unwrap().take());
        failure(failed.poll(Duration::ZERO).unwrap(), ErrorCode::Internal);
    }
    #[test]
    fn same_tick_partial_failure_refreezes_and_capture_does_not_drain_mutations() {
        let mut inspector = Inspector::new(InspectionConfig::controlled("runtime", "test"));
        let mut app = App::new();
        app.world_mut().insert_resource(3_u32);
        inspector.register_async_capture_handler(1, 1, |app, id, completion| {
            completion.complete(Ok(result(
                &id,
                &app.world().resource::<u32>().unwrap().to_string(),
            )));
            Ok(())
        });
        let first = capture(&mut inspector, &mut app, "first")
            .poll(Duration::ZERO)
            .unwrap();
        inspector
            .register_command::<serde_json::Value>(
                titan_protocol::CommandMetadata {
                    name: "partial".into(),
                    description: String::new(),
                    arguments: Default::default(),
                },
                |app, _| {
                    app.world_mut().insert_resource(4_u32);
                    Err(ProtocolError::new(
                        ErrorCode::InvalidValue,
                        "partial failure",
                    ))
                },
            )
            .unwrap();
        // A real failed command leaves committed partial state without a success revision.
        let failed = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "fail",
                Request::Invoke {
                    name: "partial".into(),
                    arguments: Default::default(),
                },
            ),
        );
        failure(failed, ErrorCode::InvalidValue);
        let deferred = app
            .world_mut()
            .commands()
            .spawn_with(crate::Name::new("queued"));
        let second = capture(&mut inspector, &mut app, "second")
            .poll(Duration::ZERO)
            .unwrap();
        assert!(app.world().get::<crate::Name>(deferred).is_none());
        app.apply_deferred().unwrap();
        assert!(app.world().get::<crate::Name>(deferred).is_some());
        assert_eq!(first.observed_frame, second.observed_frame);
        assert_eq!(first.state_revision, second.state_revision);
        let ResponseOutcome::Success {
            response: Response::Capture(first),
        } = first.outcome
        else {
            panic!("first")
        };
        let ResponseOutcome::Success {
            response: Response::Capture(second),
        } = second.outcome
        else {
            panic!("second")
        };
        assert_ne!(first.identity.capture_id, second.identity.capture_id);
        assert_eq!(first.artifact, "3");
        assert_eq!(second.artifact, "4");
    }
}
