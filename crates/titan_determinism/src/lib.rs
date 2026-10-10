#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]

extern crate alloc;

mod hints;

pub use hints::{AmbiguityHint, ChangeLocationHint, Hints};
pub use titan_snapshot::{DiffConfig, SnapshotConfig};
pub use titan_test::InputScript;

use alloc::collections::BTreeSet;
use core::fmt;
use serde::{Deserialize, Serialize};
use titan_snapshot::{TypeFilter, WorldDiff, WorldSnapshot};
use titan_test::{ExecutorKind, Sim, SimSeed, SCRIPT_VERSION};

/// Configuration applied to runs after the reference run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Variant {
    /// Use the factory's identical configuration on every run.
    #[default]
    Repeat,
    /// Replace subsequent runs' schedule executors with multithreaded executors.
    /// The reference retains the factory's executor (normally single-threaded).
    MultiThreaded,
}

/// A serializable copy of a snapshot type filter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterSettings {
    /// Optional full type-path allow list; `None` allows all types.
    pub allow: Option<BTreeSet<String>>,
    /// Full type-path deny list, which takes precedence over the allow list.
    pub deny: BTreeSet<String>,
}

impl From<&TypeFilter> for FilterSettings {
    fn from(filter: &TypeFilter) -> Self {
        Self {
            allow: filter.allow.clone(),
            deny: filter.deny.clone(),
        }
    }
}

/// Serializable snapshot settings, since `SnapshotConfig` itself is not serde-enabled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotSettings {
    /// Component filters.
    pub components: FilterSettings,
    /// Resource filters.
    pub resources: FilterSettings,
    /// Whether generic Bevy clocks are excluded.
    pub exclude_time_resources: bool,
}

impl From<&SnapshotConfig> for SnapshotSettings {
    fn from(config: &SnapshotConfig) -> Self {
        Self {
            components: (&config.components).into(),
            resources: (&config.resources).into(),
            exclude_time_resources: config.exclude_time_resources,
        }
    }
}

impl From<SnapshotSettings> for SnapshotConfig {
    fn from(settings: SnapshotSettings) -> Self {
        Self {
            components: TypeFilter {
                allow: settings.components.allow,
                deny: settings.components.deny,
            },
            resources: TypeFilter {
                allow: settings.resources.allow,
                deny: settings.resources.deny,
            },
            exclude_time_resources: settings.exclude_time_resources,
        }
    }
}

/// Check settings retained for replaying a failure with the same scenario factory.
///
/// The factory's game code, initial world, timestep, and external state cannot be
/// serialized. Supply the same factory and build when replaying these parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenarioParameters {
    /// Seed present at construction of the diverging simulation, if any.
    pub seed: Option<u64>,
    /// Seed present at construction of the reference simulation, if any.
    pub reference_seed: Option<u64>,
    /// Requested ticks per run.
    pub ticks: u64,
    /// Requested number of runs, including the reference.
    pub runs: usize,
    /// Policy for subsequent runs.
    pub variant: Variant,
    /// Complete input recording, if supplied.
    pub script: Option<InputScript>,
    /// Captured type filters and clock policy.
    pub snapshot_config: SnapshotSettings,
    /// Numeric comparison policy (invalid tolerances are normalized to zero).
    pub diff_config: DiffConfig,
}

/// The first observed difference from the reference run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Divergence {
    /// One-based run number; the reference is run 1.
    pub run: usize,
    /// One-based completed tick count. Script event indices are zero-based.
    pub tick: u64,
    /// Observable entity, component, resource, and field differences at this tick.
    pub diff: WorldDiff,
    /// Settings needed to replay with the same scenario factory and build.
    pub parameters: ScenarioParameters,
    /// Best-effort debugging leads, not proof of the cause.
    pub hints: Hints,
}

/// Whether all requested runs agreed on their observable snapshots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[expect(
    clippy::large_enum_variant,
    reason = "Keep the report's requested Diverged(Divergence) API without an extra allocation; reports are returned once per check"
)]
pub enum DeterminismReport {
    /// No differences under the supplied capture and comparison settings.
    Deterministic {
        /// Completed runs, including the reference.
        runs: usize,
        /// Completed ticks per run.
        ticks: u64,
    },
    /// Earliest mismatch across runs; ties choose the lowest run number.
    Diverged(Divergence),
}

impl DeterminismReport {
    /// Panic with the readable report if a divergence was found.
    #[track_caller]
    pub fn assert_deterministic(&self) {
        assert!(matches!(self, Self::Deterministic { .. }), "{self}");
    }
}

impl fmt::Display for DeterminismReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deterministic { runs, ticks } => write!(
                f,
                "Deterministic: {runs} runs agreed for {ticks} ticks (observable state only)"
            ),
            Self::Diverged(divergence) => divergence.fmt(f),
        }
    }
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Nondeterminism detected: run {} diverged from run 1 at tick {} (of {})",
            self.run, self.tick, self.parameters.ticks
        )?;
        writeln!(
            f,
            "  seed: {:?}, reference seed: {:?}, variant: {:?}, runs: {}",
            self.parameters.seed,
            self.parameters.reference_seed,
            self.parameters.variant,
            self.parameters.runs
        )?;
        writeln!(
            f,
            "  script: {} events; snapshot settings: {:?}; diff settings: {:?}",
            self.parameters
                .script
                .as_ref()
                .map_or(0, |script| script.events.len()),
            self.parameters.snapshot_config,
            self.parameters.diff_config
        )?;
        writeln!(f, "\n{}", self.diff)?;
        writeln!(f, "Hints (best effort; not proof of causation):")?;
        if self.hints.ambiguities.is_empty() && self.hints.change_locations.is_empty() {
            writeln!(
                f,
                "  none available (enable track_location for last-change sites)"
            )?;
        }
        for hint in &self.hints.ambiguities {
            writeln!(
                f,
                "  ambiguous systems in {} touching {}: {} <-> {}",
                hint.schedule,
                hint.types.join(", "),
                hint.systems[0],
                hint.systems[1]
            )?;
        }
        for hint in &self.hints.change_locations {
            writeln!(
                f,
                "  last changed by (track_location): {} {}: {}",
                hint.entity.as_deref().unwrap_or("resource"),
                hint.component,
                hint.location
            )?;
        }
        Ok(())
    }
}

/// Run fresh headless simulations and compare snapshots after each tick.
///
/// Memory is O(ticks × reference snapshot size), plus one candidate snapshot and
/// the final diff. Only the reference's history is retained. Runs are sequential;
/// each candidate stops at its first mismatch or before the earliest known
/// mismatch, whichever comes first. A mismatch at tick 1 ends the entire check.
/// Tick counts bound updates, not wall-clock time: a hanging game system can hang
/// the check. Only the main app world is observed, not sub-app worlds.
pub struct DeterminismCheck<F> {
    factory: F,
    script: Option<InputScript>,
    ticks: Option<u64>,
    runs: usize,
    variant: Variant,
    snapshot_config: SnapshotConfig,
    diff_config: DiffConfig,
}

impl<F: FnMut() -> Sim> DeterminismCheck<F> {
    /// Supply a factory that returns a fresh, unticked world on every call.
    /// Mutable captures are permitted for setup but must not leak gameplay state
    /// between runs; seeds and configuration should be identical for `Repeat`.
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            script: None,
            ticks: None,
            runs: 2,
            variant: Variant::Repeat,
            snapshot_config: SnapshotConfig::default(),
            diff_config: DiffConfig::default(),
        }
    }

    /// Replay the same input on every run, using `Sim::run_script` one tick at a time.
    pub fn script(mut self, script: InputScript) -> Self {
        self.script = Some(script);
        self
    }

    /// Set the required, positive number of ticks per run.
    pub fn ticks(mut self, ticks: u64) -> Self {
        self.ticks = Some(ticks);
        self
    }

    /// Set the number of runs including the reference (at least two; default two).
    pub fn runs(mut self, runs: usize) -> Self {
        self.runs = runs;
        self
    }

    /// Choose how subsequent runs differ from the reference.
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Choose captured types; defaults to `titan_snapshot`'s noise filters.
    pub fn snapshot_config(mut self, config: SnapshotConfig) -> Self {
        self.snapshot_config = config;
        self
    }

    /// Choose comparison tolerance; defaults to exact numeric comparison.
    pub fn diff_config(mut self, config: DiffConfig) -> Self {
        self.diff_config = config;
        self
    }

    /// Execute the check, reporting the earliest diverging tick across all runs.
    ///
    /// Every candidate stops at its first mismatch. Later candidates only advance
    /// while they can beat the earliest known tick; ties choose the lower run.
    ///
    /// # Panics
    /// Panics before execution if ticks are missing/zero, runs are fewer than two,
    /// or the script version is unsupported. Panics if the factory returns an
    /// already-ticked `Sim`. Game/factory panics propagate unchanged.
    pub fn run(mut self) -> DeterminismReport {
        let ticks = self
            .ticks
            .expect("DeterminismCheck requires an explicit ticks budget");
        assert!(ticks > 0, "DeterminismCheck ticks must be positive");
        assert!(
            self.runs >= 2,
            "DeterminismCheck requires at least two runs"
        );
        if let Some(script) = &self.script {
            assert_eq!(script.version, SCRIPT_VERSION, "unsupported script version");
        }
        if !self.diff_config.float_tolerance.is_finite() || self.diff_config.float_tolerance < 0.0 {
            self.diff_config.float_tolerance = 0.0;
        }
        // Sort once, stably, so equal-tick actions retain recording order.
        // Keep the original script unchanged for the reproduction payload.
        let mut sorted_script = self.script.clone();
        if let Some(script) = &mut sorted_script {
            script.events.sort_by_key(|event| event.tick);
        }
        let mut reference = (self.factory)();
        assert_eq!(
            reference.current_tick(),
            0,
            "factory must return a fresh unticked Sim"
        );
        let reference_seed = reference
            .world()
            .get_resource::<SimSeed>()
            .map(|seed| seed.0);
        let mut snapshots = Vec::new();
        let mut playback = Playback::new(sorted_script.as_ref());
        for _ in 0..ticks {
            playback.advance(&mut reference);
            snapshots.push(WorldSnapshot::capture(
                reference.world(),
                &self.snapshot_config,
            ));
        }
        // Do not retain the reference world or input buffer while executing candidates.
        drop(playback);
        drop(reference);
        let mut earliest: Option<Divergence> = None;
        for run in 2..=self.runs {
            let mut candidate = (self.factory)();
            assert_eq!(
                candidate.current_tick(),
                0,
                "factory must return a fresh unticked Sim"
            );
            let seed = candidate
                .world()
                .get_resource::<SimSeed>()
                .map(|seed| seed.0);
            if self.variant == Variant::MultiThreaded {
                candidate = candidate.with_executor_kind(ExecutorKind::MultiThreaded);
            }
            let mut playback = Playback::new(sorted_script.as_ref());
            for reference in &snapshots {
                if earliest
                    .as_ref()
                    .is_some_and(|divergence| candidate.current_tick() + 1 >= divergence.tick)
                {
                    break;
                }
                playback.advance(&mut candidate);
                let snapshot = WorldSnapshot::capture(candidate.world(), &self.snapshot_config);
                let diff = reference.diff(&snapshot, &self.diff_config);
                if !diff.is_empty() {
                    earliest = Some(Divergence {
                        run,
                        tick: candidate.current_tick(),
                        hints: hints::collect(candidate.world(), &diff),
                        diff,
                        parameters: ScenarioParameters {
                            seed,
                            reference_seed,
                            ticks,
                            runs: self.runs,
                            variant: self.variant,
                            script: self.script.clone(),
                            snapshot_config: (&self.snapshot_config).into(),
                            diff_config: self.diff_config,
                        },
                    });
                    break;
                }
            }
            if earliest
                .as_ref()
                .is_some_and(|divergence| divergence.tick == 1)
            {
                break;
            }
        }
        earliest.map_or(
            DeterminismReport::Deterministic {
                runs: self.runs,
                ticks,
            },
            DeterminismReport::Diverged,
        )
    }
}

// Pass only the current tick's events to Sim::run_script. Calling it with the
// entire recording at every tick would repeatedly scan all events (quadratic
// for recordings with one event per tick). A fresh cursor is used for each run;
// the same Sim preserves pending automatic tap releases across these calls.
struct Playback<'a> {
    events: Option<core::iter::Peekable<core::slice::Iter<'a, titan_test::ScriptEvent>>>,
    frame: InputScript,
}

impl<'a> Playback<'a> {
    fn new(sorted_script: Option<&'a InputScript>) -> Self {
        Self {
            events: sorted_script.map(|script| script.events.iter().peekable()),
            frame: InputScript::default(),
        }
    }

    fn advance(&mut self, sim: &mut Sim) {
        let Some(events) = &mut self.events else {
            sim.tick();
            return;
        };
        self.frame.events.clear();
        while let Some(event) = events.next_if(|event| event.tick == sim.current_tick()) {
            self.frame.events.push(event.clone());
        }
        sim.run_script(&self.frame, sim.current_tick() + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_input::keyboard::KeyCode;
    use titan_test::{InputAction, ScriptEvent};

    #[test]
    fn playback_consumes_each_event_once_and_passes_only_current_tick_events() {
        let script = InputScript {
            events: (0..100)
                .map(|tick| ScriptEvent {
                    tick,
                    action: InputAction::Tap(KeyCode::Space.into()),
                })
                .collect(),
            ..Default::default()
        };
        let mut playback = Playback::new(Some(&script));
        let mut sim = Sim::new(|_| {});
        for remaining in (0..100).rev() {
            playback.advance(&mut sim);
            assert_eq!(playback.frame.events.len(), 1);
            assert_eq!(playback.events.as_ref().unwrap().len(), remaining);
        }
        playback.advance(&mut sim);
        assert!(playback.frame.events.is_empty());
        assert_eq!(sim.current_tick(), 101);
        assert_eq!(script.events.len(), 100);
    }

    #[test]
    fn unsupported_script_is_rejected_before_constructing_a_world() {
        let script = InputScript {
            version: SCRIPT_VERSION + 1,
            ..Default::default()
        };
        let panic = std::panic::catch_unwind(|| {
            DeterminismCheck::new(|| panic!("factory must not run"))
                .ticks(1)
                .script(script)
                .run();
        });
        let message = panic.unwrap_err().downcast::<String>().unwrap();
        assert!(message.contains("unsupported script version"));
    }
}
