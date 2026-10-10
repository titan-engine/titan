use core::fmt;
use std::{io, path::PathBuf};

use serde::Serialize;
use titan_test::{InputButton, InputScript};

use crate::Generator;

/// All input generation and execution limits needed to regenerate a case.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FuzzConfig {
    /// Input alphabet, deduplicated in first-occurrence order.
    pub buttons: Vec<InputButton>,
    /// Distribution of presses and hold lengths.
    pub generator: Generator,
    /// Maximum number of cases requested.
    pub cases: u64,
    /// Maximum ticks per generated case.
    pub ticks: u64,
    /// Maximum simulation runs during shrinking (excluding the initial replay).
    pub max_shrink_runs: u64,
}

/// A reproduction and its provenance. Tick indices are zero-based, like
/// [`titan_test::ScriptEvent`]; `ticks` is the exclusive playback endpoint.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Failure {
    /// Name of the failing rule, or `no_panics` for a panic.
    pub invariant: String,
    /// Specific violation, including a caught panic's payload when available.
    pub message: String,
    /// Script tick whose update or subsequent invariant check failed.
    pub failing_tick: u64,
    /// Best reproducing script found within the budget (original for flaky cases).
    pub script: InputScript,
    /// Number of ticks to run when replaying `script`.
    pub ticks: u64,
    /// Original case's tick limit.
    pub original_ticks: u64,
    /// Original case's event count.
    pub original_events: usize,
    /// Input generator seed (the game seed belongs in the simulation factory).
    pub seed: u64,
    /// Zero-based case index; pass this to [`Generator::generate`].
    pub case_index: u64,
    /// Original generation configuration and execution limits.
    pub config: FuzzConfig,
    /// Actual shrink runs consumed, including candidate confirmation replays.
    pub shrink_runs: u64,
    /// Destination used by [`FuzzReport::assert_ok`].
    pub path: PathBuf,
}

/// Outcome of a bounded fuzz run. No files are written by [`crate::Fuzz::run`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FuzzReport {
    /// All cases satisfied all invariants. `ticks` is the tick limit per case.
    Passed {
        /// Number of cases executed.
        cases: u64,
        /// Number of ticks executed in each case.
        ticks: u64,
    },
    /// A failure confirmed on fresh simulations and shrunk within the budget.
    Failed(Failure),
    /// Replay disagreed with an earlier run; shrinking was stopped.
    Flaky(Failure),
}

impl FuzzReport {
    /// Save a failing script as ordinary version-1 RON, returning its path.
    /// Returns `None` for a passed report. Parent directories are created.
    /// An existing file at this configured path is overwritten.
    pub fn save_script(&self) -> io::Result<Option<PathBuf>> {
        let failure = match self {
            Self::Passed { .. } => return Ok(None),
            Self::Failed(failure) | Self::Flaky(failure) => failure,
        };
        if let Some(parent) = failure.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let ron = failure.script.to_ron().map_err(io::Error::other)?;
        std::fs::write(&failure.path, ron)?;
        Ok(Some(failure.path.clone()))
    }

    /// Assert success. On failure, save the script and panic with the full
    /// report, inline RON and a regression snippet. Saving errors are included
    /// in the panic, rather than hiding the gameplay failure.
    #[track_caller]
    pub fn assert_ok(&self) {
        if matches!(self, Self::Passed { .. }) {
            return;
        }
        match self.save_script() {
            Ok(_) => panic!("{self}"),
            Err(error) => panic!("{self}\nCould not save reproduction: {error}"),
        }
    }
}

impl fmt::Display for FuzzReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed { cases, ticks } => {
                write!(f, "Fuzz passed: {cases} cases, {ticks} ticks per case")
            }
            Self::Failed(failure) => write!(f, "Invariant failed: {failure}"),
            Self::Flaky(failure) => {
                writeln!(f, "Flaky: the failure did not reproduce consistently.")?;
                writeln!(
                    f,
                    "The game may be nondeterministic; check system ordering, RNG seeds and external state."
                )?;
                write!(f, "Original failure (not a reliable regression): {failure}")
            }
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{:?}\n  {}", self.invariant, self.message)?;
        writeln!(
            f,
            "  failing tick: {} (from {} ticks / {} events to {} ticks / {} events)",
            self.failing_tick,
            self.original_ticks,
            self.original_events,
            self.ticks,
            self.script.events.len()
        )?;
        writeln!(
            f,
            "  fuzz seed: {}, case index: {} of {} (zero-based), shrink runs: {} / {}",
            self.seed,
            self.case_index,
            self.config.cases,
            self.shrink_runs,
            self.config.max_shrink_runs
        )?;
        writeln!(f, "  generator config: {:?}", self.config)?;
        writeln!(
            f,
            "\nInput script (save destination: {}):",
            self.path.display()
        )?;
        let script = self.script.to_ron().map_err(|_| fmt::Error)?;
        writeln!(f, "{script}")?;
        writeln!(
            f,
            "\nRegression replay (use your game's simulation factory):"
        )?;
        writeln!(f, "```rust")?;
        writeln!(
            f,
            "let script = titan_test::InputScript::from_ron({script:?}).unwrap();"
        )?;
        writeln!(f, "let mut sim = make_sim();")?;
        // A zero/exhausted shrink budget can leave a long original run. Stop
        // at the failing update so a later repair cannot hide the violation.
        let endpoint = self.ticks.min(self.failing_tick.saturating_add(1));
        writeln!(f, "sim.run_script(&script, {endpoint});")?;
        writeln!(f, "// Assert {:?} on sim.world() here.", self.invariant)?;
        write!(f, "```")
    }
}

pub(crate) fn path_component(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "unnamed".to_owned()
    } else {
        name
    }
}
