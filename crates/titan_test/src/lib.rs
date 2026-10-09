#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

extern crate alloc;

mod script;

pub use script::{InputAction, InputButton, InputScript, ScriptEvent, SCRIPT_VERSION};

use alloc::collections::BTreeMap;
use bevy_app::{App, PluginsState, ScheduleRunnerPlugin, TaskPoolPlugin};
use bevy_diagnostic::FrameCountPlugin;
use bevy_ecs::{
    entity::Entity,
    name::Name,
    query::{IterQueryData, QueryFilter, ReadOnlyQueryData, ReleaseStateQueryData},
    resource::Resource,
    schedule::{InternedScheduleLabel, MultiThreadedExecutor, Schedules, SingleThreadedExecutor},
    world::{World, WorldId},
};
use bevy_input::{
    keyboard::{Key, KeyboardInput, NativeKey},
    mouse::MouseButtonInput,
    ButtonState, InputPlugin,
};
use bevy_state::app::StatesPlugin;
use bevy_time::{Fixed, Real, Time, TimePlugin, TimeUpdateStrategy, Virtual};
use bevy_transform::TransformPlugin;
use core::{any::type_name, time::Duration};
use std::collections::HashSet;

/// A seed for the game's own random generator, not a global RNG.
///
/// Read this resource in `Startup` (or later) and seed your game's RNG from it.
/// [`Sim::with_seed`] is applied after plugin build/finish/cleanup, so it cannot
/// affect randomness used in those hooks. The harness itself uses no randomness.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimSeed(pub u64);

/// The execution policy for simulation schedules.
///
/// This is a harness enum: Bevy 0.20 uses executor types rather than an
/// `ExecutorKind` enum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExecutorKind {
    /// Run systems sequentially, including when Bevy's multithreading is enabled.
    #[default]
    SingleThreaded,
    /// Opt out of sequential execution using Bevy's multithreaded executor.
    /// Actual concurrency depends on the game's Bevy/task-pool feature flags.
    MultiThreaded,
}

/// An in-process, manually ticked gameplay simulation.
///
/// `current_tick` counts completed `App::update` calls. A tick advances real time
/// by the configured duration, including the first tick; startup runs on that
/// first tick, not during construction. By default, a tick and a fixed timestep
/// are both `Duration::from_secs_f64(1.0 / 60.0)`.
///
/// Sequential execution removes thread races, but is not a replacement for
/// explicit system ordering. Use `.chain()`, `.before()`, and `.after()` for
/// systems whose relative order matters. Wall-clock access, external I/O,
/// asynchronous work, unordered iteration, and unseeded RNGs can still make a
/// game nondeterministic.
pub struct Sim {
    app: App,
    tick: u64,
    input_window: Entity,
    pending_input: BTreeMap<u64, Vec<(InputButton, ButtonState)>>,
    executor: ExecutorKind,
    configured_schedules: HashSet<(WorldId, InternedScheduleLabel)>,
}

impl Sim {
    /// Build a headless app, run `setup`, then finish and clean up its plugins.
    ///
    /// The exact base is `TaskPoolPlugin`, `FrameCountPlugin`, `TimePlugin`, and
    /// `ScheduleRunnerPlugin::run_once()` (the unconditional `MinimalPlugins`),
    /// plus `InputPlugin` (keyboard/mouse), `TransformPlugin`, and `StatesPlugin`.
    /// Add gameplay plugins in `setup`; don't add `DefaultPlugins` or duplicate
    /// the base plugins. No app update runs during construction. After setup,
    /// the harness replaces frame timing and the fixed timestep with 60 Hz;
    /// use [`Self::with_fixed_dt`] to choose another test rate.
    pub fn new(setup: impl FnOnce(&mut App)) -> Self {
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            FrameCountPlugin,
            TimePlugin,
            ScheduleRunnerPlugin::run_once(),
            InputPlugin,
            TransformPlugin,
            StatesPlugin,
        ));
        setup(&mut app);
        Self::from_app(app)
    }

    /// Wrap a ready app without installing any plugins.
    ///
    /// Requires `TimePlugin`; input helpers additionally require `InputPlugin`
    /// with keyboard/mouse support. Plugins must be ready synchronously: this
    /// does not wait for async plugin initialization. Calls `finish`/`cleanup`
    /// only if needed. Installs the default manual timestep and sequential
    /// executor policy, but preserves virtual time speed and pause settings.
    /// An already-updated app keeps its existing time and fixed-loop overstep,
    /// but its frame duration and fixed timestep are replaced with 60 Hz.
    ///
    /// # Panics
    /// Panics if plugins are not ready or the time resources are missing.
    pub fn from_app(mut app: App) -> Self {
        match app.plugins_state() {
            PluginsState::Adding => panic!("Sim::from_app: plugins are not ready at tick 0"),
            PluginsState::Ready => {
                app.finish();
                app.cleanup();
            }
            PluginsState::Finished => app.cleanup(),
            PluginsState::Cleaned => {}
        }
        assert!(
            app.world().contains_resource::<Time<Real>>()
                && app.world().contains_resource::<Time<Virtual>>()
                && app.world().contains_resource::<Time<Fixed>>(),
            "Sim::from_app requires TimePlugin at tick 0"
        );
        // Bevy's first real-time update normally only initializes its clock and
        // has zero delta. Prime just the clock, without executing game systems.
        let mut real = app.world_mut().resource_mut::<Time<Real>>();
        if real.last_update().is_none() {
            real.update_with_duration(Duration::ZERO);
        }
        // A real, empty entity identifies synthetic input without requiring
        // bevy_window or creating an OS window. It is NOT a Window component.
        let input_window = app.world_mut().spawn_empty().id();
        let mut sim = Self {
            app,
            tick: 0,
            input_window,
            pending_input: BTreeMap::new(),
            executor: ExecutorKind::SingleThreaded,
            configured_schedules: HashSet::new(),
        };
        sim.set_dt(Duration::from_secs_f64(1.0 / 60.0));
        sim.configure_schedules();
        sim
    }

    /// Set both the tick duration and fixed timestep, in seconds.
    ///
    /// Raises virtual time's maximum delta if necessary to avoid clamping large
    /// ticks. Virtual time speed/pause and existing fixed overstep are preserved.
    /// Games may independently change `Time<Fixed>` to run multiple (or fewer)
    /// fixed updates per tick.
    ///
    /// # Panics
    /// Panics if `seconds` is nonfinite, nonpositive, unrepresentable as a
    /// duration, or rounds to zero nanoseconds.
    pub fn with_fixed_dt(mut self, seconds: f64) -> Self {
        assert!(
            seconds.is_finite() && seconds > 0.0,
            "invalid simulation dt {seconds} at tick {}: expected finite positive seconds",
            self.tick
        );
        let dt = Duration::try_from_secs_f64(seconds).unwrap_or_else(|error| {
            panic!(
                "invalid simulation dt {seconds} at tick {}: {error}",
                self.tick
            )
        });
        assert!(
            !dt.is_zero(),
            "simulation dt rounds to zero at tick {}",
            self.tick
        );
        self.set_dt(dt);
        self
    }

    fn set_dt(&mut self, dt: Duration) {
        let world = self.app.world_mut();
        world.insert_resource(TimeUpdateStrategy::ManualDuration(dt));
        world.resource_mut::<Time<Fixed>>().set_timestep(dt);
        let mut virtual_time = world.resource_mut::<Time<Virtual>>();
        if virtual_time.max_delta() < dt {
            virtual_time.set_max_delta(dt);
        }
    }

    /// Insert a [`SimSeed`] for your game's RNG to consume at startup or later.
    /// Calling this after ticking replaces the resource but does not reseed
    /// an RNG your game has already initialized.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.app.insert_resource(SimSeed(seed));
        self
    }

    /// Choose sequential execution (default) or opt out with multithreading.
    ///
    /// Applies to all existing main/sub-app schedules and schedules discovered
    /// before subsequent ticks. Schedules created and run *within* a system must
    /// configure their own executor, as must schedules replaced by a system
    /// under an existing label. Custom executors and their executor-local
    /// settings (including final deferred-buffer application) are replaced.
    pub fn with_executor_kind(mut self, executor: ExecutorKind) -> Self {
        self.executor = executor;
        self.configured_schedules.clear();
        self.configure_schedules();
        self
    }

    fn configure_schedules(&mut self) {
        for app in self.app.sub_apps_mut().iter_mut() {
            let world = app.world_mut();
            let id = world.id();
            if let Some(mut schedules) = world.get_resource_mut::<Schedules>() {
                for (_, schedule) in schedules.iter_mut() {
                    if self.configured_schedules.insert((id, schedule.label())) {
                        match self.executor {
                            ExecutorKind::SingleThreaded => {
                                schedule.set_executor(SingleThreadedExecutor::new());
                            }
                            ExecutorKind::MultiThreaded => {
                                schedule.set_executor(MultiThreadedExecutor::new());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Advance one tick, applying queued input before the app's input systems.
    pub fn tick(&mut self) {
        self.configure_schedules();
        if let Some(inputs) = self.pending_input.remove(&self.tick) {
            for (button, state) in inputs {
                self.send_input(button, state);
            }
        }
        self.app.update();
        self.tick = self.tick.checked_add(1).expect("simulation tick overflow");
    }

    /// Advance exactly `count` ticks.
    pub fn run_ticks(&mut self, count: u64) {
        for _ in 0..count {
            self.tick();
        }
    }

    /// Number of completed ticks, starting at zero.
    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    /// Tick until `condition` holds, returning the number of ticks advanced.
    ///
    /// Checks the initial world too (returning zero if already satisfied).
    /// For a meaningful condition label in timeout diagnostics, use
    /// [`Self::run_until_named`].
    ///
    /// # Panics
    /// Panics with the tick, condition type, and entity names if it times out.
    #[track_caller]
    pub fn run_until<C: FnMut(&World) -> bool>(&mut self, max_ticks: u64, condition: C) -> u64 {
        self.run_until_named(max_ticks, type_name::<C>(), condition)
    }

    /// Like [`Self::run_until`], with a description such as `"Player has landed"`.
    ///
    /// # Panics
    /// Panics if the condition never holds, including at the initial world.
    /// The message includes elapsed ticks, the current tick, `description`, and
    /// up to 16 entities with their [`Name`] (if any).
    #[track_caller]
    pub fn run_until_named(
        &mut self,
        max_ticks: u64,
        description: &str,
        mut condition: impl FnMut(&World) -> bool,
    ) -> u64 {
        for elapsed in 0..=max_ticks {
            if condition(self.world()) {
                return elapsed;
            }
            if elapsed < max_ticks {
                self.tick();
            }
        }
        let summary = self.entity_summary();
        panic!(
            "run_until gave up after {max_ticks} ticks at tick {}; condition: {description}; {summary}",
            self.tick
        );
    }

    /// Queue a press for the next tick; accepts [`bevy_input::keyboard::KeyCode`]
    /// or [`bevy_input::mouse::MouseButton`]. Uses real input messages, never
    /// writes `ButtonInput` directly. The button stays held until released.
    /// Messages identify a spawned empty entity, not a `Window` or a placeholder
    /// entity. Consumers requiring window components must provide their own
    /// input messages. Keyboard messages carry an unidentified logical key, no
    /// text, and `repeat = false`: this API tests physical keys, not text input.
    pub fn press(&mut self, button: impl Into<InputButton>) {
        self.queue_input(self.tick, button.into(), ButtonState::Pressed);
    }

    /// Queue a release for the next tick.
    pub fn release(&mut self, button: impl Into<InputButton>) {
        self.queue_input(self.tick, button.into(), ButtonState::Released);
    }

    /// Press, advance one tick, then queue a release for the following tick.
    ///
    /// Immediately after this call the button is still pressed/just-pressed.
    /// Call [`Self::tick`] to observe just-released; another tick clears it.
    /// Tapping an already-held button still releases it. Consecutive taps can
    /// produce both edges in one frame (queued release, then a new press).
    /// Edges are per-frame: zero/multiple fixed steps can miss/repeat them if
    /// your game changes the fixed timestep independently of the frame duration.
    pub fn tap(&mut self, button: impl Into<InputButton>) {
        let button = button.into();
        self.press(button);
        self.tick();
        self.release(button);
    }

    fn queue_input(&mut self, tick: u64, button: InputButton, state: ButtonState) {
        self.pending_input
            .entry(tick)
            .or_default()
            .push((button, state));
    }

    fn send_input(&mut self, button: InputButton, state: ButtonState) {
        match button {
            InputButton::Key(key_code) => {
                self.app
                    .world_mut()
                    .write_message(KeyboardInput {
                        key_code,
                        // This API scripts physical keys, not a locale/text layout.
                        logical_key: Key::Unidentified(NativeKey::Unidentified),
                        state,
                        text: None,
                        repeat: false,
                        window: self.input_window,
                    })
                    .unwrap_or_else(|| {
                        panic!("keyboard input requires InputPlugin at tick {}", self.tick)
                    });
            }
            InputButton::Mouse(button) => {
                self.app
                    .world_mut()
                    .write_message(MouseButtonInput {
                        button,
                        state,
                        window: self.input_window,
                    })
                    .unwrap_or_else(|| {
                        panic!("mouse input requires InputPlugin at tick {}", self.tick)
                    });
            }
        }
    }

    /// Play script events up to the exclusive absolute endpoint `until_tick`.
    ///
    /// Tick N's actions are injected when `current_tick() == N`, before update
    /// N. Equal-tick events retain file order; automatic tap releases occur
    /// before that next tick's scripted actions. Scripted taps do not add ticks.
    /// Past events are ignored, allowing continuation with the same script.
    /// A tap at the endpoint's last tick leaves its release queued for the next
    /// tick, including when playback resumes with [`Self::tick`].
    ///
    /// # Panics
    /// Panics before playback for unsupported versions or an endpoint earlier
    /// than the current tick.
    #[track_caller]
    pub fn run_script(&mut self, script: &InputScript, until_tick: u64) {
        assert_eq!(
            script.version, SCRIPT_VERSION,
            "unsupported script version at tick {}",
            self.tick
        );
        assert!(
            until_tick >= self.tick,
            "script endpoint {until_tick} precedes current tick {}",
            self.tick
        );
        let mut events: Vec<_> = script
            .events
            .iter()
            .filter(|event| event.tick >= self.tick && event.tick < until_tick)
            .collect();
        events.sort_by_key(|event| event.tick);
        let mut events = events.into_iter().peekable();
        while self.tick < until_tick {
            while let Some(event) = events.next_if(|event| event.tick == self.tick) {
                match event.action {
                    InputAction::Press(button) => self.press(button),
                    InputAction::Release(button) => self.release(button),
                    InputAction::Tap(button) => {
                        self.press(button);
                        self.queue_input(self.tick + 1, button, ButtonState::Released);
                    }
                }
            }
            self.tick();
        }
    }

    /// Inspect the simulation world without advancing time.
    pub fn world(&self) -> &World {
        self.app.world()
    }

    /// Mutate game state directly. For scripted input, prefer the input helpers.
    ///
    /// Invalidates the schedule-policy cache so replacement schedules are
    /// configured before the next tick.
    pub fn world_mut(&mut self) -> &mut World {
        self.configured_schedules.clear();
        self.app.world_mut()
    }

    /// Fetch a resource, failing with its type and the current tick if missing.
    #[track_caller]
    pub fn resource<R: Resource>(&self) -> &R {
        self.world().get_resource::<R>().unwrap_or_else(|| {
            panic!(
                "missing resource {} at tick {}",
                type_name::<R>(),
                self.tick
            )
        })
    }

    /// Fetch exactly one matching entity's read-only query data.
    ///
    /// Supports ordinary component references, tuples, and query data that
    /// implement [`ReleaseStateQueryData`]. Use [`Self::world_mut`] for mutable
    /// or state-borrowing queries.
    ///
    /// # Panics
    /// Panics on zero/multiple matches with the tick, query/filter types, match
    /// count, and up to 16 matching entity IDs and names.
    #[track_caller]
    pub fn single<D, F>(&mut self) -> D::Item<'_, 'static>
    where
        D: ReadOnlyQueryData + IterQueryData + ReleaseStateQueryData,
        F: QueryFilter,
    {
        let tick = self.tick;
        let world = self.app.world_mut();
        let mut query = world.query_filtered::<(Entity, D, Option<&Name>), F>();
        let count = query.iter(world).count();
        assert_eq!(
            count,
            1,
            "single failed at tick {tick}; query {}, filter {}: {count} entities matched: {}",
            type_name::<D>(),
            type_name::<F>(),
            query
                .iter(world)
                .take(16)
                .map(|(entity, _, name)| describe_entity(entity, name))
                .collect::<Vec<_>>()
                .join(", ")
        );
        D::release_state(query.single(world).expect("match count checked").1)
    }

    /// Collect matching read-only query data, borrowing components from the world.
    ///
    /// Iteration order is Bevy's query order, not sorted by entity or name.
    /// Supports query data implementing [`ReleaseStateQueryData`].
    pub fn query<D, F>(&mut self) -> Vec<D::Item<'_, 'static>>
    where
        D: ReadOnlyQueryData + IterQueryData + ReleaseStateQueryData,
        F: QueryFilter,
    {
        let world = self.app.world_mut();
        world
            .query_filtered::<D, F>()
            .iter(world)
            .map(D::release_state)
            .collect()
    }

    fn entity_summary(&mut self) -> String {
        let world = self.app.world_mut();
        let mut query = world.query::<(Entity, Option<&Name>)>();
        let count = query.iter(world).count();
        // Bevy also stores internal system entities in the world. Prefer named
        // game entities so those internal entities don't hide useful context.
        let mut entities: Vec<_> = query.iter(world).collect();
        entities.sort_by_key(|(entity, name)| (name.is_none(), entity.to_bits()));
        let names = entities
            .into_iter()
            .take(16)
            .map(|(entity, name)| describe_entity(entity, name))
            .collect::<Vec<_>>()
            .join(", ");
        format!("world has {count} entities (showing up to 16, named first): {names}")
    }
}

fn describe_entity(entity: Entity, name: Option<&Name>) -> String {
    match name {
        Some(name) => format!("{entity} {name:?}"),
        None => entity.to_string(),
    }
}
