#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod generator;
mod report;
mod shrink;

pub use generator::Generator;
pub use report::{Failure, FuzzConfig, FuzzReport};

use bevy_ecs::world::World;
use bevy_transform::components::Transform;
use std::{
    any::{Any, TypeId},
    panic::AssertUnwindSafe,
    path::PathBuf,
};
use titan_test::{InputAction, InputButton, InputScript, Sim};

struct Invariant<'a> {
    name: String,
    check: Box<dyn Fn(&World) -> Result<(), String> + 'a>,
}

/// A bounded, deterministic input fuzzer. Every case and every replay calls
/// `factory` to obtain a fresh simulation; never reuse a world or external
/// mutable gameplay state. The factory also controls the game's own RNG seed.
///
/// All limits count completed simulation updates/runs, not wall-clock time:
/// a factory, game system or invariant that hangs cannot be interrupted.
pub struct Fuzz<'a, F> {
    factory: F,
    config: FuzzConfig,
    seed: u64,
    invariants: Vec<Invariant<'a>>,
    test_name: String,
    output_dir: PathBuf,
}

impl<'a, F: Fn() -> Sim> Fuzz<'a, F> {
    /// Start with 100 cases of 300 ticks, seed 0 and 500 shrink runs.
    /// An explicit nonempty input alphabet must be supplied with [`Self::buttons`].
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            config: FuzzConfig {
                buttons: Vec::new(),
                generator: Generator::default(),
                cases: 100,
                ticks: 300,
                max_shrink_runs: 500,
            },
            seed: 0,
            invariants: Vec::new(),
            test_name: std::thread::current().name().unwrap_or("fuzz").to_owned(),
            output_dir: PathBuf::from("target/titan_fuzz"),
        }
    }

    /// Set the input alphabet. Accepts keys, mouse buttons, or mixed
    /// [`InputButton`] values. Duplicates are removed in first-occurrence order.
    pub fn buttons<B: Into<InputButton>>(mut self, buttons: impl IntoIterator<Item = B>) -> Self {
        self.config.buttons = generator::unique_buttons(buttons.into_iter().map(Into::into));
        self
    }

    /// Add a named rule checked after every tick, starting with tick 0 (the
    /// first update runs `Startup`). No check runs on the uninitialized world.
    /// Names must be unique; `no_panics` is reserved for implicit panic catching.
    /// Keep checks deterministic and free of external mutable state.
    ///
    /// # Panics
    /// Panics if the name is empty, reserved or already registered.
    pub fn invariant(
        mut self,
        name: impl Into<String>,
        check: impl Fn(&World) -> Result<(), String> + 'a,
    ) -> Self {
        let name = name.into();
        assert!(
            !name.is_empty() && name != "no_panics",
            "invalid invariant name {name:?}"
        );
        assert!(
            !self.invariants.iter().any(|rule| rule.name == name),
            "duplicate invariant name {name:?}"
        );
        self.invariants.push(Invariant {
            name,
            check: Box::new(check),
        });
        self
    }

    /// Reject NaN and infinity in any translation, rotation or scale component
    /// of any [`Transform`]. Does not check `GlobalTransform` or other components.
    pub fn no_nan_transforms(self) -> Self {
        self.invariant("no_nan_transforms", |world| {
            for entity in world.iter_entities() {
                if let Some(transform) = entity.get::<Transform>()
                    && (!transform.translation.is_finite()
                        || !transform.rotation.is_finite()
                        || !transform.scale.is_finite())
                {
                    return Err(format!(
                        "entity {:?} has non-finite Transform: {transform:?}",
                        entity.id()
                    ));
                }
            }
            Ok(())
        })
    }

    /// Set the maximum number of generated cases. Zero executes no cases.
    pub fn cases(mut self, cases: u64) -> Self {
        self.config.cases = cases;
        self
    }

    /// Set ticks per case. Zero builds each world but executes no updates or
    /// world invariants; factory panics still count as failures at tick 0.
    pub fn ticks(mut self, ticks: u64) -> Self {
        self.config.ticks = ticks;
        self
    }

    /// Set the input seed, independent of [`Sim::with_seed`].
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Set the press/hold distribution, validated when running.
    pub fn generator(mut self, generator: Generator) -> Self {
        self.config.generator = generator;
        self
    }

    /// Limit simulation runs during shrinking. Confirmation runs count too.
    /// The original case's mandatory reproducibility replay is not included.
    /// Zero disables shrinking, but still checks for a flaky original failure.
    pub fn max_shrink_runs(mut self, runs: u64) -> Self {
        self.config.max_shrink_runs = runs;
        self
    }

    /// Override the test directory name (defaults to the current thread name).
    /// It is sanitized into one filesystem component, as are invariant names.
    pub fn test_name(mut self, name: impl Into<String>) -> Self {
        self.test_name = name.into();
        self
    }

    /// Override the output root. Files are saved by [`FuzzReport::assert_ok`],
    /// not by [`Self::run`], to `<root>/<test-name>/<invariant>.ron`.
    pub fn output_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.output_dir = path.into();
        self
    }

    /// Execute cases until the first failure, replay it, then shrink it.
    /// Panics from construction, updates or invariant checks are caught under
    /// `panic = "unwind"`. The usual panic hook still runs; aborts cannot be caught.
    ///
    /// # Panics
    /// Panics for an empty alphabet or invalid generator configuration.
    pub fn run(self) -> FuzzReport {
        assert!(
            !self.config.buttons.is_empty(),
            "set a nonempty input alphabet with buttons(...)"
        );
        self.config.generator.validate();
        for case_index in 0..self.config.cases {
            let script = self.config.generator.generate(
                &self.config.buttons,
                self.config.ticks,
                self.seed,
                case_index,
            );
            let Some(violation) = self.execute(&script, self.config.ticks) else {
                continue;
            };
            let mut failure = Failure {
                invariant: violation.name.clone(),
                message: violation.message.clone(),
                failing_tick: violation.tick,
                original_events: script.events.len(),
                script,
                ticks: self.config.ticks,
                original_ticks: self.config.ticks,
                seed: self.seed,
                case_index,
                config: self.config.clone(),
                shrink_runs: 0,
                path: self
                    .output_dir
                    .join(report::path_component(&self.test_name))
                    .join(format!("{}.ron", report::path_component(&violation.name))),
            };
            if self.execute(&failure.script, failure.ticks).as_ref() != Some(&violation) {
                return FuzzReport::Flaky(failure);
            }
            // Arbitrary panic_any payloads cannot be compared by value. Even
            // equal types might represent unrelated violations, so preserve
            // the original input rather than minimize to an opaque panic.
            if violation.opaque_panic.is_some() {
                return FuzzReport::Failed(failure);
            }
            let original = failure.clone();
            if shrink::shrink(&mut failure, |script, ticks| self.execute(script, ticks)) {
                // Do not publish a known-unreliable minimized reproduction.
                let mut original = original;
                original.shrink_runs = failure.shrink_runs;
                return FuzzReport::Flaky(original);
            }
            return FuzzReport::Failed(failure);
        }
        FuzzReport::Passed {
            cases: self.config.cases,
            ticks: self.config.ticks,
        }
    }

    fn execute(&self, script: &InputScript, ticks: u64) -> Option<Violation> {
        let mut current_tick = 0;
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut sim = (self.factory)();
            assert_eq!(
                sim.current_tick(),
                0,
                "fuzz factory must return a fresh, unticked Sim"
            );
            let mut events = script.events.iter().peekable();
            for tick in 0..ticks {
                current_tick = tick;
                while let Some(event) = events.next_if(|event| event.tick == tick) {
                    match event.action {
                        InputAction::Press(button) => sim.press(button),
                        InputAction::Release(button) => sim.release(button),
                        InputAction::Tap(_) => {
                            unreachable!("fuzzer generates only press/release events")
                        }
                    }
                }
                sim.tick();
                for invariant in &self.invariants {
                    if let Err(message) = (invariant.check)(sim.world()) {
                        return Some(Violation {
                            name: invariant.name.clone(),
                            message,
                            tick,
                            opaque_panic: None,
                        });
                    }
                }
            }
            None
        }));
        match outcome {
            Ok(violation) => violation,
            Err(payload) => Some(Violation {
                name: "no_panics".to_owned(),
                message: panic_message(payload.as_ref()),
                tick: current_tick,
                opaque_panic: if payload.is::<String>() || payload.is::<&str>() {
                    None
                } else {
                    Some(payload.as_ref().type_id())
                },
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Violation {
    name: String,
    message: String,
    tick: u64,
    // Used only in-process to detect differing payload types on the original
    // replay. Unknown values still cannot safely be compared or shrunk.
    opaque_panic: Option<TypeId>,
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "panic with a non-string payload (shrinking disabled: opaque payload)".to_owned()
    }
}
