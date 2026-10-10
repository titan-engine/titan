//! Opt-in assertions against explicitly managed golden files.

use std::{
    fmt::{self, Write},
    fs,
    io::ErrorKind,
    path::Path,
};
use titan_snapshot::{
    DiffConfig, EntityMatchConfig, EntityMatching, EntitySnapshot, SnapshotConfig, WorldSnapshot,
};

use crate::Sim;

/// Capture, comparison, and entity selection for a golden-file assertion.
///
/// Available with the `snapshots` feature. Defaults use `SnapshotConfig::default()`
/// and exact, ID-based comparison. For cross-run golden files, explicitly choose
/// name or stable-component matching and select only relevant game entities.
/// Type filters alone do not remove entities (even empty ones).
#[derive(Clone, Debug, Default)]
pub struct SnapshotAssertConfig {
    /// Component/resource filters applied when capturing the current world.
    pub snapshot: SnapshotConfig,
    /// Numeric tolerance and entity identity strategy.
    pub comparison: EntityMatchConfig,
    /// Optional selection applied to captured entities, before writing/comparing.
    ///
    /// For example, retain only entities containing a gameplay component or with
    /// a particular name. This excludes harness entities without weakening the
    /// matcher's missing/duplicate-key diagnostics for selected entities.
    /// Saved files are not re-filtered: changing selection requires an update.
    /// References to excluded entities cannot be normalized by name/key.
    pub entity_filter: Option<fn(&EntitySnapshot) -> bool>,
}

impl SnapshotAssertConfig {
    /// Use these component/resource capture filters with default comparison.
    pub fn new(snapshot: SnapshotConfig) -> Self {
        Self {
            snapshot,
            ..Default::default()
        }
    }

    /// Configure absolute floating-point tolerance.
    pub fn with_diff(mut self, diff: DiffConfig) -> Self {
        self.comparison.diff = diff;
        self
    }

    /// Match entities by ID, name, or a captured, reflected stable component.
    pub fn with_entity_matching(mut self, matching: EntityMatching) -> Self {
        self.comparison.entity_matching = matching;
        self
    }

    /// Retain only selected entities in the current capture.
    pub fn with_entity_filter(mut self, filter: fn(&EntitySnapshot) -> bool) -> Self {
        self.entity_filter = Some(filter);
        self
    }
}

impl Sim {
    /// Compare the current world with a pretty-JSON golden file.
    ///
    /// `path` is explicit: relative paths are resolved against the working
    /// directory. Prefer `Path::new(env!("CARGO_MANIFEST_DIR")).join(...)` in tests.
    /// Does not advance the simulation or mutate the world.
    ///
    /// Only `TITAN_UPDATE_SNAPSHOTS=1` enables writing: creates parent directories
    /// and writes/overwrites the golden, without asserting. Otherwise the file
    /// must already exist. Review and commit updates; never set this variable in
    /// CI. Types must be registered with reflection to observe their fields.
    ///
    /// # Panics
    /// Panics on missing, unreadable, or invalid golden files, write errors, or
    /// observable differences/matching diagnostics. The message includes the
    /// current test thread's name, tick, path, and a readable matched WorldDiff,
    /// limited to 80 lines and 8 KiB of diff text.
    #[track_caller]
    pub fn assert_snapshot(&self, path: impl AsRef<Path>, config: &SnapshotAssertConfig) {
        let update = std::env::var_os("TITAN_UPDATE_SNAPSHOTS").is_some_and(|value| value == "1");
        self.assert_snapshot_mode(path.as_ref(), config, update);
    }

    // An explicit mode lets unit tests exercise writes without changing global
    // process environment (unsafe in Rust 2024 and racy with parallel tests).
    #[track_caller]
    fn assert_snapshot_mode(&self, path: &Path, config: &SnapshotAssertConfig, update: bool) {
        let thread = std::thread::current();
        let test = thread.name().unwrap_or("<unnamed test>");
        let context = format!(
            "snapshot assertion for test {test:?} at tick {} ({})",
            self.current_tick(),
            path.display()
        );
        let mut actual = WorldSnapshot::capture(self.world(), &config.snapshot);
        if let Some(filter) = config.entity_filter {
            actual.entities.retain(|_, entity| filter(entity));
        }
        if update {
            let mut json = serde_json::to_string_pretty(&actual)
                .unwrap_or_else(|error| panic!("{context}: cannot serialize snapshot: {error}"));
            json.push('\n');
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)
                    .unwrap_or_else(|error| panic!("{context}: cannot create directory: {error}"));
            }
            fs::write(path, json)
                .unwrap_or_else(|error| panic!("{context}: cannot write golden file: {error}"));
            return;
        }
        let json = fs::read_to_string(path).unwrap_or_else(|error| {
            if error.kind() == ErrorKind::NotFound {
                panic!(
                    "{context}: missing golden file. Re-run this test with \
                     TITAN_UPDATE_SNAPSHOTS=1 to create it, then review and commit the JSON."
                );
            }
            panic!("{context}: cannot read golden file: {error}");
        });
        let expected: WorldSnapshot = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("{context}: invalid golden JSON: {error}"));
        let diff = expected.diff_matched(&actual, &config.comparison);
        assert!(
            diff.is_empty(),
            "{context}: world differs from golden file:\n{}\n\
             To intentionally update it, re-run this test with TITAN_UPDATE_SNAPSHOTS=1 \
             and review the JSON.",
            limited_diff(&diff)
        );
    }
}

fn limited_diff(diff: &impl fmt::Display) -> String {
    struct Limited {
        text: String,
        lines: usize,
        truncated: bool,
    }
    impl Write for Limited {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            for ch in text.chars() {
                if self.truncated || self.text.len() + ch.len_utf8() > 8192 || self.lines >= 80 {
                    self.truncated = true;
                    break;
                }
                self.text.push(ch);
                if ch == '\n' {
                    self.lines += 1;
                }
            }
            Ok(())
        }
    }
    let mut output = Limited {
        text: String::new(),
        lines: 0,
        truncated: false,
    };
    write!(output, "{diff}").expect("writing to a String cannot fail");
    if output.truncated {
        output.text.push_str(
            "\n... diff truncated (80 lines / 8 KiB); compare the golden JSON for full state.",
        );
    }
    output.text
}

#[cfg(test)]
mod tests {
    use super::limited_diff;

    #[test]
    fn golden_diff_is_bounded_and_utf8_safe() {
        let output = limited_diff(&"é".repeat(10_000));
        assert!(output.starts_with('é'));
        assert!(output.contains("diff truncated"));
        assert!(output.len() < 8400);
        let output = limited_diff(&"line\n".repeat(1000));
        assert_eq!(output.matches("line\n").count(), 80);
        assert!(output.contains("diff truncated"));
        assert_eq!(limited_diff(&"small diff"), "small diff");
    }
}
