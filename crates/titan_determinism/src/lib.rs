#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]

extern crate alloc;

mod hints;
mod shuffle;

pub use hints::{AmbiguityHint, ChangeLocationHint, Hints};
pub use titan_snapshot::{DiffConfig, EntityMatching, SnapshotConfig};
pub use titan_test::InputScript;

use alloc::collections::BTreeSet;
use core::fmt;
use serde::{Deserialize, Serialize};
use titan_snapshot::{
    EntityMatch, MatchDiagnostic, MatchedWorldDiff, TypeFilter, WorldDiff, WorldSnapshot,
};
use titan_test::{ExecutorKind, Sim, SimSeed};

/// Configuration applied to runs after the reference run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Variant {
    /// Use the factory's identical configuration on every run.
    #[default]
    Repeat,
    /// Replace subsequent runs' schedule executors with multithreaded executors.
    /// The reference retains the factory's executor (normally single-threaded).
    MultiThreaded,
    /// Seed subsequent runs' topological system order with a single-threaded
    /// executor. The reference remains unchanged. The same seed is reused in
    /// every candidate and schedule; it is independent of gameplay `SimSeed`.
    ShuffleAmbiguous {
        /// Seed for reproducible, cycle-safe choices of system ordering.
        seed: u64,
    },
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
    /// Entity identity strategy used for every compared tick.
    #[serde(default)]
    pub entity_matching: EntityMatching,
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
    /// Logical keys and both world-local IDs, including unchanged pairs.
    #[serde(default)]
    pub matches: Vec<EntityMatch>,
    /// Missing, opaque, or duplicate keys; these also count as divergence.
    #[serde(default)]
    pub diagnostics: Vec<MatchDiagnostic>,
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
        /// Entity identity strategy used for every compared tick.
        #[serde(default)]
        entity_matching: EntityMatching,
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
            Self::Deterministic { runs, ticks, entity_matching } => write!(
                f,
                "Deterministic: {runs} runs agreed for {ticks} ticks (observable state only; matcher: {entity_matching:?})"
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
        writeln!(f, "  entity matcher: {:?}", self.parameters.entity_matching)?;
        // Reuse snapshot's side-aware key/diagnostic formatting. The structural
        // diff stays directly accessible for existing report consumers.
        writeln!(
            f,
            "\n{}",
            MatchedWorldDiff {
                diff: self.diff.clone(),
                matches: self.matches.clone(),
                diagnostics: self.diagnostics.clone(),
            }
        )?;
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
            if let Some(order) = &hint.order {
                writeln!(f, "    shuffled order: {} -> {}", order[0], order[1])?;
            }
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
    entity_matching: EntityMatching,
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
            entity_matching: EntityMatching::default(),
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

    /// Choose entity identity, defaulting to full index/generation IDs.
    ///
    /// Use `ByName` for unique, stable names or `ByComponent` for a captured,
    /// reflected stable key. These ignore allocation order and normalize typed
    /// entity references. Missing, opaque, or duplicate keys cause divergence;
    /// there is no ID fallback. ID matching is stricter: it also detects spawn
    /// order differences. Key modes build identity maps and normalize snapshot
    /// copies each tick; they do not use an ID-ordered hash shortcut.
    pub fn entity_matching(mut self, matching: EntityMatching) -> Self {
        self.entity_matching = matching;
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
    /// already-ticked `Sim`, or if `ShuffleAmbiguous` cannot find the standard
    /// Bevy `Main` driver, encounters an already-initialized schedule it has not
    /// configured (unless it is system-free), or discovers overridden shuffle
    /// settings on a live nonempty schedule.
    /// New/replacement schedules must be available before their first run.
    /// Game/factory panics propagate unchanged.
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
            script.validate();
        }
        if !self.diff_config.float_tolerance.is_finite() || self.diff_config.float_tolerance < 0.0 {
            self.diff_config.float_tolerance = 0.0;
        }
        let match_config = self
            .diff_config
            .with_entity_matching(self.entity_matching.clone());
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
            let shuffle_seed = match self.variant {
                Variant::ShuffleAmbiguous { seed } => {
                    shuffle::install(candidate.world_mut(), seed);
                    candidate = candidate.with_executor_kind(ExecutorKind::SingleThreaded);
                    Some(seed)
                }
                _ => None,
            };
            let mut playback = Playback::new(sorted_script.as_ref());
            for reference in &snapshots {
                if earliest
                    .as_ref()
                    .is_some_and(|divergence| candidate.current_tick() + 1 >= divergence.tick)
                {
                    break;
                }
                playback.advance(&mut candidate);
                if let Some(seed) = shuffle_seed {
                    // Reject in-tick unsupported schedules before returning any
                    // result or allowing Sim's next policy pass to reset them.
                    shuffle::validate(candidate.world(), seed);
                }
                let snapshot = WorldSnapshot::capture(candidate.world(), &self.snapshot_config);
                // Preserve the inexpensive default comparison. Only build its
                // identity metadata on failure. Key modes compare every tick
                // after matching/normalization, so reordered IDs cannot obscure
                // the first actual gameplay divergence.
                let matched = if self.entity_matching == EntityMatching::ById {
                    if reference.diff(&snapshot, &self.diff_config).is_empty() {
                        continue;
                    }
                    reference.diff_matched(&snapshot, &match_config)
                } else {
                    reference.diff_matched(&snapshot, &match_config)
                };
                if !matched.is_empty() {
                    let hints = hints::collect_matched(candidate.world(), &matched, shuffle_seed);
                    earliest = Some(Divergence {
                        run,
                        tick: candidate.current_tick(),
                        hints,
                        diff: matched.diff,
                        matches: matched.matches,
                        diagnostics: matched.diagnostics,
                        parameters: ScenarioParameters {
                            seed,
                            reference_seed,
                            ticks,
                            runs: self.runs,
                            variant: self.variant,
                            script: self.script.clone(),
                            snapshot_config: (&self.snapshot_config).into(),
                            diff_config: self.diff_config,
                            entity_matching: self.entity_matching.clone(),
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
                entity_matching: self.entity_matching,
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
    use titan_test::{InputAction, ScriptEvent, SCRIPT_VERSION};

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
    fn both_supported_script_versions_replay_through_determinism() {
        use bevy_app::Update;
        use bevy_ecs::prelude::*;
        use bevy_input::{
            gamepad::{Gamepad, GamepadAxis, GamepadButton},
            mouse::AccumulatedMouseMotion,
            ButtonInput,
        };

        for (source, gamepad_input) in [
            (
                "(version:1,events:[(tick:0,action:Press(Key(Space)))])",
                false,
            ),
            (
                "(version:2,events:[
                (tick:0,action:Press(Gamepad(slot:0,button:South))),
                (tick:0,action:SetAxis(slot:0,axis:LeftStickX,value:0.75)),
                (tick:0,action:MouseMotion(x:12.0,y:-3.0))])",
                true,
            ),
        ] {
            let script = InputScript::from_ron(source).unwrap();
            let report = DeterminismCheck::new(|| {
                Sim::new(|app| {
                    app.add_systems(
                        Update,
                        move |keys: Res<ButtonInput<KeyCode>>,
                              pads: Query<&Gamepad>,
                              motion: Res<AccumulatedMouseMotion>| {
                            if gamepad_input {
                                let pad = pads.single().unwrap();
                                assert!(pad.pressed(GamepadButton::South));
                                assert_eq!(pad.get(GamepadAxis::LeftStickX), Some(0.75));
                                assert_eq!((motion.delta.x, motion.delta.y), (12.0, -3.0));
                            } else {
                                assert!(keys.pressed(KeyCode::Space));
                                assert!(pads.is_empty());
                            }
                        },
                    );
                })
            })
            .ticks(1)
            .script(script)
            .run();
            report.assert_deterministic();
        }
    }

    #[test]
    fn version_one_gamepad_actions_are_rejected_before_constructing_a_world() {
        let script = InputScript::from_ron(
            "(version:1,events:[(tick:0,action:Press(Gamepad(slot:0,button:South)))])",
        )
        .unwrap();
        let panic = std::panic::catch_unwind(|| {
            DeterminismCheck::new(|| panic!("factory must not run"))
                .ticks(1)
                .script(script)
                .run();
        });
        let message = panic.unwrap_err().downcast::<&str>().unwrap();
        assert!(message.contains("require script version 2"));
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
