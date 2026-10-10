use core::fmt;
use std::path::PathBuf;

use bevy_ecs::world::World;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use titan_test::Sim;

use crate::{report, ActionRng, Failure, Fuzz, FuzzReport, FuzzScript, Violation};

/// One tick's game-defined actions. Missing ticks apply the script's idle action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionTick<A> {
    /// Zero-based simulation update index.
    pub tick: u64,
    /// Complete action value to apply before this update.
    pub action: A,
}

/// Versioned, standalone action regression, including its exclusive endpoint.
/// Each tick applies exactly one value: its event, or `idle` if absent. An
/// adapter must overwrite held input as well as one-shot input on every tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionScript<A> {
    /// Currently supported format version: 1.
    pub version: u32,
    /// Number of simulation updates to replay.
    pub ticks: u64,
    /// Neutral actions, applied on ticks whose events were removed by shrinking.
    pub idle: A,
    /// Events in strictly increasing tick order, all before `ticks`.
    pub events: Vec<ActionTick<A>>,
}

impl<A: Serialize + DeserializeOwned> ActionScript<A> {
    /// Serialize a validated standalone regression as readable RON.
    pub fn to_ron(&self) -> Result<String, String> {
        self.validate()?;
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|error| error.to_string())
    }

    /// Parse RON, rejecting unsupported versions, duplicate/unsorted ticks and
    /// events outside the playback endpoint before any simulation is touched.
    pub fn from_ron(source: &str) -> Result<Self, String> {
        let script: Self = ron::from_str(source).map_err(|error| error.to_string())?;
        script.validate()?;
        Ok(script)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!(
                "unsupported action script version {}",
                self.version
            ));
        }
        if self.events.iter().any(|event| event.tick >= self.ticks)
            || self
                .events
                .windows(2)
                .any(|pair| pair[0].tick >= pair[1].tick)
        {
            return Err("action ticks must be strictly increasing and before the endpoint".into());
        }
        Ok(())
    }

    /// Replay without a fuzzer. Game/update/adapter panics propagate normally.
    /// Can resume a partially replayed simulation; earlier events are ignored.
    ///
    /// # Panics
    /// Panics before playback for invalid scripts or a simulation past `ticks`.
    pub fn replay(&self, sim: &mut Sim, apply: impl Fn(&mut World, &A)) {
        self.validate().expect("invalid action script");
        assert!(
            sim.current_tick() <= self.ticks,
            "action endpoint precedes simulation tick"
        );
        let start = sim.current_tick();
        let mut events = self
            .events
            .iter()
            .filter(|event| event.tick >= start)
            .peekable();
        while sim.current_tick() < self.ticks {
            let action = events
                .next_if(|event| event.tick == sim.current_tick())
                .map_or(&self.idle, |event| &event.action);
            apply(sim.world_mut(), action);
            sim.tick();
        }
    }
}

impl<A: Serialize + DeserializeOwned> FuzzScript for ActionScript<A> {
    fn reproduction_ron(&self, endpoint: u64) -> Result<String, String> {
        self.validate()?;
        let script = ActionScript {
            version: self.version,
            ticks: endpoint.min(self.ticks),
            idle: &self.idle,
            events: self
                .events
                .iter()
                .filter(|event| event.tick < endpoint)
                .map(|event| ActionTick {
                    tick: event.tick,
                    action: &event.action,
                })
                .collect(),
        };
        ron::ser::to_string_pretty(&script, ron::ser::PrettyConfig::default())
            .map_err(|error| error.to_string())
    }

    fn event_count(&self) -> usize {
        self.events.len()
    }

    fn replay_snippet(&self, f: &mut fmt::Formatter<'_>, ron: &str, _endpoint: u64) -> fmt::Result {
        writeln!(
            f,
            "let script = titan_fuzz::ActionScript::<YourActions>::from_ron({ron:?}).unwrap();"
        )?;
        writeln!(f, "let mut sim = make_sim();")?;
        writeln!(f, "script.replay(&mut sim, apply_actions);")
    }
}

/// Execution limits recorded with an action-layer finding. The generator and
/// adapter are user code; keep those and the factory with the regression test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ActionConfig {
    /// Maximum generated cases.
    pub cases: u64,
    /// Maximum updates per case.
    pub ticks: u64,
    /// Candidate runs, including confirmation, allowed during shrinking.
    pub max_shrink_runs: u64,
}

type Simplifier<'a, A> = Box<dyn Fn(&A) -> Vec<A> + 'a>;

/// Fuzz game actions rather than raw buttons. `generate` receives an independent
/// seeded stream per case and is called once per tick. `apply` runs before every
/// update (including tick 0, before Startup), and must replace all action state.
/// `idle` is the neutral value used when shrinking removes a tick's actions.
/// Factories, adapters, generators and invariants must be deterministic and
/// free of external mutable gameplay state. Limits cannot interrupt hangs.
pub struct ActionFuzz<'a, F, G, P, A> {
    runner: Fuzz<'a, F>,
    generate: G,
    apply: P,
    idle: A,
    simplify: Option<Simplifier<'a, A>>,
}

impl<'a, F, G, P, A> ActionFuzz<'a, F, G, P, A>
where
    F: Fn() -> Sim,
    G: Fn(&mut ActionRng) -> A,
    P: Fn(&mut World, &A),
    A: Clone + Serialize + DeserializeOwned,
{
    /// Start with 100 cases of 300 ticks, seed 0 and 500 shrink runs.
    pub fn new(factory: F, idle: A, generate: G, apply: P) -> Self {
        Self {
            runner: Fuzz::new(factory),
            generate,
            apply,
            idle,
            simplify: None,
        }
    }

    /// Add a named rule checked after every update. Names follow [`Fuzz::invariant`].
    pub fn invariant(
        mut self,
        name: impl Into<String>,
        check: impl Fn(&World) -> Result<(), String> + 'a,
    ) -> Self {
        self.runner = self.runner.invariant(name, check);
        self
    }

    /// Reject non-finite transforms, using [`Fuzz::no_nan_transforms`].
    pub fn no_nan_transforms(mut self) -> Self {
        self.runner = self.runner.no_nan_transforms();
        self
    }

    /// Set maximum generated cases. Zero generates and executes nothing.
    pub fn cases(mut self, cases: u64) -> Self {
        self.runner = self.runner.cases(cases);
        self
    }

    /// Set updates per case. Zero still constructs each fresh simulation.
    pub fn ticks(mut self, ticks: u64) -> Self {
        self.runner = self.runner.ticks(ticks);
        self
    }

    /// Set action generation seed, independent of the game's RNG seed.
    pub fn seed(mut self, seed: u64) -> Self {
        self.runner = self.runner.seed(seed);
        self
    }

    /// Bound shrink runs; zero still performs the mandatory original replay.
    pub fn max_shrink_runs(mut self, runs: u64) -> Self {
        self.runner = self.runner.max_shrink_runs(runs);
        self
    }

    /// Set the sanitized regression directory name.
    pub fn test_name(mut self, name: impl Into<String>) -> Self {
        self.runner = self.runner.test_name(name);
        self
    }

    /// Set the root used by [`FuzzReport::save_script`] and [`FuzzReport::assert_ok`].
    pub fn output_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.runner = self.runner.output_dir(path);
        self
    }

    /// Supply finite, ordered candidates that simplify one action (for example,
    /// move an axis toward zero). Called once per remaining event after sequence
    /// shrinking. Candidates are tried in order, accepting the first confirmed
    /// reproduction. Panics in generation/simplification are configuration errors
    /// and propagate, unlike panics in the factory, adapter, game or invariants.
    pub fn simplify_action(mut self, simplify: impl Fn(&A) -> Vec<A> + 'a) -> Self {
        self.simplify = Some(Box::new(simplify));
        self
    }

    /// Generate, check, confirm and shrink the first finding. No files are
    /// written until explicitly saving or asserting the returned report.
    pub fn run(self) -> FuzzReport<ActionScript<A>, ActionConfig> {
        let config = ActionConfig {
            cases: self.runner.config.cases,
            ticks: self.runner.config.ticks,
            max_shrink_runs: self.runner.config.max_shrink_runs,
        };
        for case_index in 0..config.cases {
            let mut rng = ActionRng::new(self.runner.seed, case_index);
            let script = ActionScript {
                version: 1,
                ticks: config.ticks,
                idle: self.idle.clone(),
                events: (0..config.ticks)
                    .map(|tick| ActionTick {
                        tick,
                        action: (self.generate)(&mut rng),
                    })
                    .collect(),
            };
            let Some(violation) = self.execute(&script) else {
                continue;
            };
            let mut failure = Failure {
                invariant: violation.name.clone(),
                message: violation.message.clone(),
                failing_tick: violation.tick,
                ticks: script.ticks,
                original_ticks: script.ticks,
                original_events: script.events.len(),
                script,
                seed: self.runner.seed,
                case_index,
                config,
                shrink_runs: 0,
                path: self
                    .runner
                    .output_dir
                    .join(report::path_component(&self.runner.test_name))
                    .join(format!("{}.ron", report::path_component(&violation.name))),
            };
            if self.execute(&failure.script).as_ref() != Some(&violation) {
                return FuzzReport::Flaky(failure);
            }
            if violation.opaque_panic.is_some() {
                return FuzzReport::Failed(failure);
            }
            let original = failure.clone();
            let mut shrinker = ActionShrinker {
                fuzz: &self,
                failure: &mut failure,
                flaky: false,
            };
            shrinker.shrink();
            if shrinker.flaky {
                let mut original = original;
                original.shrink_runs = failure.shrink_runs;
                return FuzzReport::Flaky(original);
            }
            return FuzzReport::Failed(failure);
        }
        FuzzReport::Passed {
            cases: config.cases,
            ticks: config.ticks,
        }
    }

    fn execute(&self, script: &ActionScript<A>) -> Option<Violation> {
        let mut events = script.events.iter().peekable();
        self.runner.execute_with(script.ticks, |sim, tick| {
            let action = events
                .next_if(|event| event.tick == tick)
                .map_or(&script.idle, |event| &event.action);
            (self.apply)(sim.world_mut(), action);
        })
    }
}

struct ActionShrinker<'f, 'a, F, G, P, A> {
    fuzz: &'f ActionFuzz<'a, F, G, P, A>,
    failure: &'f mut Failure<ActionScript<A>, ActionConfig>,
    flaky: bool,
}

impl<F, G, P, A> ActionShrinker<'_, '_, F, G, P, A>
where
    F: Fn() -> Sim,
    G: Fn(&mut ActionRng) -> A,
    P: Fn(&mut World, &A),
    A: Clone + Serialize + DeserializeOwned,
{
    fn available(&self) -> bool {
        !self.flaky && self.failure.shrink_runs < self.failure.config.max_shrink_runs
    }

    fn consider(&mut self, mut script: ActionScript<A>) -> bool {
        if !self.available() {
            return false;
        }
        self.failure.shrink_runs += 1;
        let Some(violation) = self.fuzz.execute(&script) else {
            return false;
        };
        if violation.name != self.failure.invariant
            || violation.opaque_panic.is_some()
            || (violation.name == "no_panics" && violation.message != self.failure.message)
            || !self.available()
        {
            return false;
        }
        script.ticks = script.ticks.min(violation.tick.saturating_add(1));
        script.events.retain(|event| event.tick < script.ticks);
        self.failure.shrink_runs += 1;
        if self.fuzz.execute(&script).as_ref() != Some(&violation) {
            self.flaky = true;
            return false;
        }
        self.failure.ticks = script.ticks;
        self.failure.script = script;
        self.failure.failing_tick = violation.tick;
        self.failure.message = violation.message;
        true
    }

    fn shrink(&mut self) {
        let mut script = self.failure.script.clone();
        script.ticks = script
            .ticks
            .min(self.failure.failing_tick.saturating_add(1));
        script.events.retain(|event| event.tick < script.ticks);
        self.consider(script);

        // Delta debugging deletes action chunks, leaving explicit neutral ticks.
        let mut chunk = self.failure.script.events.len().div_ceil(2).max(1);
        loop {
            let mut index = 0;
            let mut removed = false;
            while index < self.failure.script.events.len() && self.available() {
                let mut script = self.failure.script.clone();
                let end = (index + chunk).min(script.events.len());
                script.events.drain(index..end);
                if self.consider(script) {
                    removed = true;
                } else {
                    index += chunk;
                }
            }
            if !self.available() || (chunk == 1 && !removed) {
                break;
            }
            chunk = chunk.div_ceil(2);
        }

        // Move actions earlier, without stacking multiple complete values on a tick.
        let mut index = 0;
        while index < self.failure.script.events.len() {
            let mut cursor = index;
            let mut step = (self.failure.script.events[cursor].tick / 2).max(1);
            loop {
                if cursor >= self.failure.script.events.len() {
                    break;
                }
                let tick = self.failure.script.events[cursor].tick;
                if tick == 0 || !self.available() {
                    break;
                }
                let target = tick.saturating_sub(step);
                let mut script = self.failure.script.clone();
                let occupied = script.events.iter().any(|event| event.tick == target);
                script.events[cursor].tick = target;
                script.events.sort_by_key(|event| event.tick);
                if occupied || !self.consider(script) {
                    if step == 1 {
                        break;
                    }
                    step = step.div_ceil(2);
                } else if let Some(moved) = self
                    .failure
                    .script
                    .events
                    .iter()
                    .position(|event| event.tick == target)
                {
                    cursor = moved;
                } else {
                    break;
                }
            }
            index += 1;
        }

        if let Some(simplify) = &self.fuzz.simplify {
            let mut index = 0;
            while index < self.failure.script.events.len() {
                if !self.available() {
                    break;
                }
                for action in simplify(&self.failure.script.events[index].action) {
                    let mut script = self.failure.script.clone();
                    script.events[index].action = action;
                    if self.consider(script) || !self.available() {
                        break;
                    }
                }
                index += 1;
            }
        }
    }
}
