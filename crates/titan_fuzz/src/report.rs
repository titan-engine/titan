use core::fmt;
use std::{io, path::PathBuf};

use serde::Serialize;
use titan_test::{InputButton, InputScript};

use crate::Generator;

/// A saved fuzz reproduction. Implemented by button and action scripts.
pub trait FuzzScript {
    /// Serialize a standalone regression ending at the failure's exclusive
    /// endpoint, not its report metadata. Button scripts use an external endpoint.
    fn reproduction_ron(&self, endpoint: u64) -> Result<String, String>;
    /// Number of action events in the reproduction.
    fn event_count(&self) -> usize;
    /// Print a replay snippet using `make_sim()` and the exclusive endpoint.
    fn replay_snippet(&self, f: &mut fmt::Formatter<'_>, ron: &str, endpoint: u64) -> fmt::Result;
}

impl FuzzScript for InputScript {
    fn reproduction_ron(&self, _endpoint: u64) -> Result<String, String> {
        self.to_ron().map_err(|error| error.to_string())
    }

    fn event_count(&self) -> usize {
        self.events.len()
    }

    fn replay_snippet(&self, f: &mut fmt::Formatter<'_>, ron: &str, endpoint: u64) -> fmt::Result {
        writeln!(
            f,
            "let script = titan_test::InputScript::from_ron({ron:?}).unwrap();"
        )?;
        writeln!(f, "let mut sim = make_sim();")?;
        writeln!(f, "sim.run_script(&script, {endpoint});")
    }
}

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
/// Defaults describe button fuzzing; [`crate::ActionFuzz`] uses
/// [`crate::ActionScript`] and [`crate::ActionConfig`] instead.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Failure<S = InputScript, C = FuzzConfig> {
    /// Name of the failing rule, or `no_panics` for a panic.
    pub invariant: String,
    /// Specific violation, including a caught panic's payload when available.
    pub message: String,
    /// Script tick whose update or subsequent invariant check failed.
    pub failing_tick: u64,
    /// Best reproducing script found within the budget (original for flaky cases).
    pub script: S,
    /// Number of ticks to run when replaying `script`.
    pub ticks: u64,
    /// Original case's tick limit.
    pub original_ticks: u64,
    /// Original case's event count.
    pub original_events: usize,
    /// Input generator seed (the game seed belongs in the simulation factory).
    pub seed: u64,
    /// Zero-based case index; pass this to [`Generator::generate`] or
    /// [`crate::ActionRng::new`] with the recorded seed.
    pub case_index: u64,
    /// Original generation configuration and execution limits.
    pub config: C,
    /// Actual shrink runs consumed, including candidate confirmation replays.
    pub shrink_runs: u64,
    /// Destination used by [`FuzzReport::assert_ok`].
    pub path: PathBuf,
}

/// Outcome of a bounded fuzz run. Neither [`crate::Fuzz::run`] nor
/// [`crate::ActionFuzz::run`] writes files. Default type parameters describe
/// button scripts; action campaigns infer their own script/configuration types.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum FuzzReport<S = InputScript, C = FuzzConfig> {
    /// All cases satisfied all invariants. `ticks` is the tick limit per case.
    Passed {
        /// Number of cases executed.
        cases: u64,
        /// Number of ticks executed in each case.
        ticks: u64,
    },
    /// A failure confirmed on fresh simulations and shrunk within the budget.
    Failed(Failure<S, C>),
    /// Replay disagreed with an earlier run; shrinking was stopped.
    Flaky(Failure<S, C>),
}

impl<S: FuzzScript, C: fmt::Debug> FuzzReport<S, C> {
    /// Save a failing button or action script as versioned RON, returning its path.
    /// Returns `None` for a passed report. Parent directories are created.
    /// An existing file at this configured path is overwritten. Action scripts
    /// save only the prefix through the failing update, even without shrinking;
    /// the report retains the original provenance.
    pub fn save_script(&self) -> io::Result<Option<PathBuf>> {
        let failure = match self {
            Self::Passed { .. } => return Ok(None),
            Self::Failed(failure) | Self::Flaky(failure) => failure,
        };
        if let Some(parent) = failure.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let endpoint = failure.ticks.min(failure.failing_tick.saturating_add(1));
        let ron = failure
            .script
            .reproduction_ron(endpoint)
            .map_err(io::Error::other)?;
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

impl<S: FuzzScript, C: fmt::Debug> fmt::Display for FuzzReport<S, C> {
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

impl<S: FuzzScript, C: fmt::Debug> fmt::Display for Failure<S, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{:?}\n  {}", self.invariant, self.message)?;
        writeln!(
            f,
            "  failing tick: {} (from {} ticks / {} events to {} ticks / {} events)",
            self.failing_tick,
            self.original_ticks,
            self.original_events,
            self.ticks,
            self.script.event_count()
        )?;
        writeln!(
            f,
            "  fuzz seed: {}, case index: {} (zero-based), shrink runs: {}",
            self.seed, self.case_index, self.shrink_runs
        )?;
        writeln!(f, "  generator config: {:?}", self.config)?;
        writeln!(
            f,
            "\nInput script (save destination: {}):",
            self.path.display()
        )?;
        let endpoint = self.ticks.min(self.failing_tick.saturating_add(1));
        let script = match self.script.reproduction_ron(endpoint) {
            Ok(script) => script,
            Err(error) => return writeln!(f, "Could not serialize reproduction: {error}"),
        };
        writeln!(f, "{script}")?;
        writeln!(
            f,
            "\nRegression replay (use your game's simulation factory):"
        )?;
        writeln!(f, "```rust")?;
        // A zero/exhausted shrink budget can leave a long original run. Stop
        // at the failing update so a later repair cannot hide the violation.
        self.script.replay_snippet(f, &script, endpoint)?;
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
