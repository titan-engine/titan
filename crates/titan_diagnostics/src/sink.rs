use crate::{DiagnosticReport, DiagnosticsPlugin, FailureContext, RecentLog};
use alloc::collections::VecDeque;
use bevy_ecs::error::ErrorHandler;
use core::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs,
    io::{self, Write},
    sync::{Mutex, MutexGuard, PoisonError},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Sink {
    config: DiagnosticsPlugin,
    pub previous: ErrorHandler,
    session: String,
    state: Mutex<State>,
}

#[derive(Default)]
pub(crate) struct State {
    pub frame: Option<u32>,
    pub schedules: Vec<(u64, std::thread::ThreadId, String)>,
    pub logs: VecDeque<RecentLog>,
    reports: VecDeque<DiagnosticReport>,
    sequence: u64,
    pub last_write_error: Option<String>,
}

pub(crate) fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

impl Sink {
    pub fn new(config: DiagnosticsPlugin, previous: ErrorHandler) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            config,
            previous,
            session: session_id(nanos, std::process::id()),
            state: Mutex::new(State::default()),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn log(&self, log: RecentLog) {
        let mut state = self.lock();
        if self.config.max_recent_logs == 0 {
            return;
        }
        if state.logs.len() == self.config.max_recent_logs {
            state.logs.pop_front();
        }
        state.logs.push_back(log);
    }

    pub fn record(
        &self,
        kind: &str,
        severity: Option<String>,
        context: Option<FailureContext>,
        message: String,
        backtrace: Option<String>,
        location: Option<String>,
    ) {
        let mut state = self.lock();
        let now = unix_ms();
        let schedule = crate::layer::current_schedule().or_else(|| {
            // Executor workers have detached system spans. A process-global
            // panic from an unrelated thread has no ECS context and must not
            // inherit the app's schedule just because it happens to be active.
            context.as_ref()?;
            // Use the caller's schedule only when active spans have one owner.
            let (_, thread, name) = state.schedules.last()?;
            state
                .schedules
                .iter()
                .all(|(_, owner, _)| owner == thread)
                .then(|| name.clone())
        });
        let frame = state.frame;
        let recent_logs = state.logs.iter().cloned().collect();
        let existing = state.reports.iter().position(|report| {
            report.kind == kind
                && report.severity == severity
                && same_context(report.context.as_ref(), context.as_ref())
                && report.schedule == schedule
                && message_key(kind, &report.message) == message_key(kind, &message)
                && report.location == location
        });
        let report = if let Some(index) = existing {
            let mut report = state.reports.remove(index).unwrap();
            report.count = report.count.saturating_add(1);
            report.last_seen_unix_ms = now;
            report.frame = frame;
            report.context = context;
            report.message = message;
            report.backtrace = backtrace;
            report.recent_logs = recent_logs;
            report
        } else {
            state.sequence += 1;
            DiagnosticReport {
                schema_version: 1,
                id: format!("{}-{:016x}", self.session, state.sequence),
                kind: kind.into(),
                severity,
                app_name: self.config.app_name.clone(),
                app_version: self.config.app_version.clone(),
                context,
                schedule,
                frame,
                first_frame: frame,
                message,
                backtrace,
                location,
                count: 1,
                first_seen_unix_ms: now,
                last_seen_unix_ms: now,
                recent_logs,
            }
        };
        // IO is serialized with error recording, but no tracing or user handler
        // is invoked while holding this lock (including on write failure).
        state.last_write_error = self.persist(&report).err().map(|error| error.to_string());
        state.reports.push_back(report);
        while state.reports.len() > self.config.max_reports {
            state.reports.pop_front();
        }
    }

    fn persist(&self, report: &DiagnosticReport) -> io::Result<()> {
        fs::create_dir_all(&self.config.directory)?;
        let path = self
            .config
            .directory
            .join(format!("titan-diagnostics-{}.json", report.id));
        let mut temp = tempfile::NamedTempFile::new_in(&self.config.directory)?;
        serde_json::to_writer_pretty(&mut temp, report)?;
        temp.write_all(b"\n")?;
        temp.as_file().sync_all()?;
        temp.persist(&path).map_err(|error| error.error)?;

        // Only regular files with our exact filename grammar are eligible.
        // Retention is by modification time so a deduped report stays recent.
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.config.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            if owned_filename(&name.to_string_lossy()) && entry.file_type()?.is_file() {
                files.push((entry.metadata()?.modified()?, entry.path()));
            }
        }
        files.sort();
        let remove_count = files.len().saturating_sub(self.config.max_reports);
        for (_, old) in files
            .into_iter()
            .filter(|(_, old)| old != &path)
            .take(remove_count)
        {
            fs::remove_file(old)?;
        }
        Ok(())
    }
}

fn session_id(nanos: u128, pid: u32) -> String {
    let counter = NEXT_SESSION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .expect("diagnostics session counter exhausted");
    // Preserve the 32-hex session grammar, but clock precision, rollback and
    // pre-epoch fallback cannot collide within this process: the low half is
    // a checked monotonic counter, independent of the timestamp's low 64 bits.
    let time = nanos & u128::from(u64::MAX);
    format!("{time:016x}{counter:016x}-{pid:08x}")
}

fn owned_filename(name: &str) -> bool {
    let Some(id) = name
        .strip_prefix("titan-diagnostics-")
        .and_then(|s| s.strip_suffix(".json"))
    else {
        return false;
    };
    let parts: Vec<_> = id.split('-').collect();
    parts.len() == 3
        && parts.iter().zip([32, 8, 16]).all(|(part, len)| {
            part.len() == len && part.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn same_context(a: Option<&FailureContext>, b: Option<&FailureContext>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            a.kind == b.kind && a.name == b.name && a.system == b.system && a.on_set == b.on_set
        }
        (None, None) => true,
        _ => false,
    }
}

// Bevy's Display appends an origin backtrace when features are unified by other
// crates. Ignore that stack for deduplication, retaining it in the actual report.
fn message_key<'a>(kind: &str, message: &'a str) -> &'a str {
    if kind != "error" {
        return message;
    }
    const FILTER_NOTE: &str = "note: Some \"noisy\" backtrace lines have been filtered out. Run with `BEVY_BACKTRACE=full` for a verbose backtrace.";
    // An embedded Rust backtrace is error content, not necessarily Bevy's
    // origin stack. Only consider the LAST capture boundary and verify that
    // its entire suffix is a stack containing Bevy error construction evidence.
    let mut offset = 0;
    let mut full_start = None;
    for line in message.split_inclusive('\n') {
        if stack_frame(line).is_some_and(|(index, symbol)| {
            index == 0
                && (symbol.contains("std::backtrace_rs::backtrace::")
                    || symbol.contains("std::backtrace::Backtrace::capture")
                    || symbol.contains("<std::backtrace::Backtrace>::capture"))
        }) {
            full_start = Some(offset);
        }
        offset += line.len();
    }
    if let Some(start) = full_start
        && is_full_bevy_stack(&message[start..])
    {
        return &message[..start];
    }
    if !message.trim_end().ends_with(FILTER_NOTE) {
        return message;
    }
    offset = message.len();
    let mut has_location = false;
    let mut constructor_location = false;
    let mut later_index = None;
    for line in message.split_inclusive('\n').rev() {
        offset -= line.len();
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some((index, symbol)) = stack_frame(line) {
            if later_index.is_some_and(|later| index >= later)
                || symbol.contains('/')
                || symbol.contains('\\')
                || !(has_location || symbol.contains("::") || symbol.contains('<'))
            {
                break;
            }
            // Stop at the verified construction frame, never walk into payload
            // frames preceding it. Any remaining capture/construction prefix is
            // stable across repetitions and is safe to leave in the key.
            if constructor_location || bevy_constructor(symbol) {
                return &message[..offset];
            }
            later_index = Some(index);
            has_location = false;
            constructor_location = false;
        } else if line.starts_with("             at ") {
            has_location = true;
            constructor_location = bevy_constructor(line);
        } else if !(line.is_empty() || line == FILTER_NOTE) {
            break;
        }
    }
    message
}

fn stack_frame(line: &str) -> Option<(u32, &str)> {
    let (index, symbol) = line.split_once(": ")?;
    if index.len() < 4 || !index.starts_with(' ') {
        return None;
    }
    Some((index.trim().parse().ok()?, symbol))
}

fn bevy_constructor(line: &str) -> bool {
    line.contains("bevy_ecs/src/error/bevy_error.rs")
        || line.contains("bevy_ecs\\src\\error\\bevy_error.rs")
        || line.contains("bevy_ecs::error::bevy_error::BevyError::")
        || (line.contains("<bevy_ecs::error::bevy_error::BevyError as ") && line.contains("::from"))
}

fn is_full_bevy_stack(stack: &str) -> bool {
    let mut constructor = false;
    let mut previous_index = None;
    for line in stack.lines() {
        if let Some((index, _)) = stack_frame(line) {
            if previous_index.is_some_and(|previous| index <= previous) {
                return false;
            }
            previous_index = Some(index);
        } else if !(line.is_empty() || line.starts_with("             at ")) {
            return false;
        }
        constructor |= bevy_constructor(line);
    }
    constructor && previous_index.is_some()
}

#[cfg(test)]
mod tests {
    use super::session_id;

    #[test]
    fn session_ids_do_not_collide_with_equal_or_pre_epoch_clock_readings() {
        assert_ne!(session_id(123, 42), session_id(123, 42));
        assert_ne!(session_id(0, 42), session_id(0, 42));
        assert_ne!(session_id(123, 42), session_id(0, 42));
    }
}
