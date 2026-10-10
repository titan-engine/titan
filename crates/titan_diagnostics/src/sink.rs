use crate::{DiagnosticReport, DiagnosticsPlugin, FailureContext, RecentLog};
use alloc::collections::VecDeque;
use bevy_ecs::error::ErrorHandler;
use std::{
    fs,
    io::{self, Write},
    sync::{Mutex, MutexGuard, PoisonError},
    time::{SystemTime, UNIX_EPOCH},
};

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
            session: format!("{nanos:032x}-{:08x}", std::process::id()),
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
    // Require Bevy's filter marker or std's full backtrace capture symbols.
    // Ordinary numbered error text and panic payloads must remain verbatim.
    let mut offset = 0;
    for line in message.split_inclusive('\n') {
        if stack_symbol(line).is_some_and(|symbol| {
            symbol.contains("std::backtrace_rs::backtrace::")
                || symbol.contains("std::backtrace::Backtrace::capture")
                || symbol.contains("<std::backtrace::Backtrace>::capture")
        }) {
            // Full stacks have a recognizable capture boundary. Never walk
            // backwards from it into numbered application payload lines.
            return &message[..offset];
        }
        offset += line.len();
    }
    if !message.trim_end().ends_with(FILTER_NOTE) {
        return message;
    }
    offset = message.len();
    let mut first_frame = None;
    let mut has_location = false;
    for line in message.split_inclusive('\n').rev() {
        offset -= line.len();
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(symbol) = stack_symbol(line) {
            // Short names need a following source location. Arbitrary asset
            // paths/identifiers are not sufficient evidence of a stack frame.
            if symbol.contains('/')
                || symbol.contains('\\')
                || !(has_location || symbol.contains("::") || symbol.contains('<'))
            {
                break;
            }
            first_frame = Some(offset);
            has_location = false;
        } else if line.starts_with("             at ") {
            has_location = true;
        } else if !(line.is_empty() || line == FILTER_NOTE) {
            break;
        }
    }
    &message[..first_frame.unwrap_or(message.len())]
}

fn stack_symbol(line: &str) -> Option<&str> {
    let (index, symbol) = line.split_once(": ")?;
    (index.len() >= 4
        && index.starts_with(' ')
        && !index.trim().is_empty()
        && index.trim().bytes().all(|byte| byte.is_ascii_digit()))
    .then_some(symbol)
}
