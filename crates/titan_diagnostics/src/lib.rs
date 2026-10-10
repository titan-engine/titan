//! Structured local error and panic reports, without changing error policy.
//!
//! Install [`DiagnosticsLayer`] on your tracing subscriber as well as
//! [`DiagnosticsPlugin`] on your app to capture warnings and schedule labels.

#![doc = include_str!("../README.md")]

extern crate alloc;

mod layer;
mod report;
mod sink;

pub use layer::DiagnosticsLayer;
pub use report::{DiagnosticReport, FailureContext, RecentLog};

use alloc::sync::{Arc, Weak};
use bevy_app::{App, Main, Plugin};
use bevy_diagnostic::{FrameCount, FrameCountPlugin};
use bevy_ecs::{
    error::{BevyError, ErrorContext, ErrorHandler, FallbackErrorHandler},
    prelude::*,
};
use core::cell::Cell;
use std::{
    backtrace::{Backtrace, BacktraceStatus},
    path::PathBuf,
    sync::{Mutex, Once, PoisonError},
};

use sink::Sink;

static ACTIVE: Mutex<Weak<Sink>> = Mutex::new(Weak::new());
static INSTALL_HOOK: Once = Once::new();
thread_local! {
    static REPORTING: Cell<bool> = const { Cell::new(false) };
}

/// Local report configuration. The directory must not contain sensitive files
/// named `titan-diagnostics-*.json`: those names are reserved for retention.
#[derive(Debug, Clone)]
pub struct DiagnosticsPlugin {
    /// Output directory, created lazily on the first report.
    pub directory: PathBuf,
    /// Maximum retained reports, including reports from earlier runs. Must be nonzero.
    pub max_reports: usize,
    /// Maximum number of recent WARN/ERROR events. Zero disables log retention.
    pub max_recent_logs: usize,
    /// Application identity copied into each report.
    pub app_name: String,
    /// Optional application version copied into each report.
    pub app_version: Option<String>,
}

impl Default for DiagnosticsPlugin {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("diagnostics"),
            max_reports: 32,
            max_recent_logs: 64,
            app_name: "app".into(),
            app_version: None,
        }
    }
}

/// Owns the process-global sink. Dropping the app releases ownership; a second
/// simultaneously live diagnostics app is rejected rather than silently routed
/// to another app's directory. Do not remove this resource while running systems.
#[derive(Resource)]
pub struct DiagnosticsState(Arc<Sink>);

impl DiagnosticsState {
    /// The most recent reporting/output failure, if any. Reporting failures never change
    /// the previous error handler's policy and are not logged recursively.
    pub fn last_write_error(&self) -> Option<String> {
        self.0.lock().last_write_error.clone()
    }
}

impl Plugin for DiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        assert!(self.max_reports > 0, "max_reports must be nonzero");
        let previous = app
            .world()
            .get_resource::<FallbackErrorHandler>()
            .copied()
            .unwrap_or_default()
            .0;
        assert!(
            !core::ptr::fn_addr_eq(previous, report_error as ErrorHandler),
            "the diagnostics handler must not be copied to another app"
        );
        let sink = Arc::new(Sink::new(self.clone(), previous));
        {
            let mut active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
            if active.upgrade().is_some() {
                drop(active);
                panic!("only one live DiagnosticsPlugin app is supported per process");
            }
            *active = Arc::downgrade(&sink);
        }
        app.insert_resource(DiagnosticsState(sink))
            .insert_resource(FallbackErrorHandler(report_error));
        if !app.is_plugin_added::<FrameCountPlugin>() {
            app.add_plugins(FrameCountPlugin);
        }
        app.add_systems(Main, refresh_frame.before(Main::run_main));
        INSTALL_HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if let Some(sink) = active_sink() {
                    reporting(|| {
                        let message = info
                            .payload()
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| info.payload().downcast_ref::<&str>().map(|s| (*s).into()))
                            .unwrap_or_else(|| "non-string panic payload".into());
                        sink.record(
                            "panic",
                            Some("Panic".into()),
                            layer::panic_context(),
                            message,
                            capture_backtrace(),
                            info.location().map(ToString::to_string),
                        );
                    });
                }
                previous(info);
            }));
        });
    }
}

/// Refresh the frame snapshot before executing schedules outside `App::update`.
/// Normal main-app updates call this automatically before `Main::run_main`.
/// An absent `FrameCount` is represented as null, not a guessed frame number.
pub fn refresh_frame(world: &mut World) {
    if let Some(state) = world.get_resource::<DiagnosticsState>() {
        state.0.lock().frame = world.get_resource::<FrameCount>().map(|frame| frame.0);
    }
}

fn active_sink() -> Option<Arc<Sink>> {
    ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .upgrade()
}

fn reporting(f: impl FnOnce()) {
    let _ = REPORTING.try_with(|flag| {
        if flag.replace(true) {
            return;
        }
        struct Reset<'a>(&'a Cell<bool>);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _reset = Reset(flag);
        f();
    });
}

pub(crate) fn format_field(value: &dyn core::fmt::Debug) -> String {
    REPORTING
        .try_with(|flag| {
            // The hook still chains to the previous hook, but must not mistake a
            // diagnostics visitor's formatter failure for a new app panic report.
            let previous = flag.replace(true);
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| format!("{value:?}")));
            let value = match result {
                Ok(value) => value,
                Err(payload) => {
                    discard_panic_payload(payload);
                    "<diagnostics: field formatter failed>".into()
                }
            };
            flag.set(previous);
            value
        })
        .unwrap_or_else(|_| "<diagnostics: field formatter unavailable>".into())
}

fn discard_panic_payload(payload: Box<dyn core::any::Any + Send>) {
    // A panic_any payload can itself have a panicking Drop implementation.
    // Drop it behind another boundary, leaking only a secondary panic payload
    // if destruction fails: recursively dropping that payload is not safe.
    if let Err(secondary) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)))
    {
        core::mem::forget(secondary);
    }
}

fn capture_backtrace() -> Option<String> {
    let trace = Backtrace::capture();
    (trace.status() == BacktraceStatus::Captured).then(|| trace.to_string())
}

fn report_error(error: BevyError, context: ErrorContext) {
    let Some(sink) = active_sink() else {
        // This can only occur if the ownership resource was manually removed.
        (FallbackErrorHandler::default().0)(error, context);
        return;
    };
    // Display is arbitrary user code. A broken formatter must not replace the
    // original error with a diagnostics panic before the previous policy runs.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reporting(|| {
            sink.record(
                "error",
                Some(format!("{:?}", error.severity())),
                Some(FailureContext::from(&context)),
                error.to_string(),
                capture_backtrace(),
                None,
            );
        });
    }));
    if let Err(payload) = result {
        reporting(|| discard_panic_payload(payload));
        sink.lock().last_write_error = Some("diagnostic formatting or recording panicked".into());
    }
    // Never hold sink or registry locks across arbitrary user error policy.
    (sink.previous)(error, context);
}
