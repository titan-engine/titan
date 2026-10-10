# `titan_test`: your first headless gameplay test

`titan_test` drives a real Bevy `App` with controlled time and input. Test your
systems and game plugins without opening a window, creating a renderer, loading
GPU assets, or sleeping between frames. It is a separate crate: Bevy's existing
APIs and crate names are unchanged.

## Run the tiny jumping game

From the Titan workspace, run:

```sh
cargo test -p titan_test --test gameplay
```

[`tests/gameplay.rs`](tests/gameplay.rs) is a complete, runnable game and test,
not pseudocode. Its `JumpGamePlugin` spawns one `Jumper` in `Startup` and runs
jump, gravity, and landing logic in `FixedUpdate`. The test:

1. Creates a simulation and installs only the game plugin.
2. Advances one tick to spawn the player and checks that it is grounded.
3. Taps Space and checks that the player leaves the ground.
4. Waits at most 120 ticks for an actual landing, with a named failure condition.
5. Checks height, velocity, jump count, and landing count, then advances again
   to check that the tap did not cause another jump.

The central part looks like this (the plugin and component definitions are in
that file):

```rust,ignore
let mut sim = Sim::new(|app| {
    app.add_plugins(JumpGamePlugin);
})
.with_fixed_dt(1.0 / 60.0);

sim.tick(); // Run Startup and one complete frame.
assert!(sim.single::<&Jumper, ()>().grounded);

sim.tap(KeyCode::Space); // Press + one frame. Release is queued for the next frame.
assert!(sim.single::<&Jumper, ()>().height > 0.0);

sim.run_until_named(120, "player lands after jumping", |world| {
    world
        .iter_entities()
        .filter_map(|entity| entity.get::<Jumper>())
        .any(|player| player.landings == 1 && player.grounded)
});
assert_eq!(sim.single::<&Jumper, ()>().height, 0.0);
```

For your own game, put the same pattern in `tests/gameplay.rs`, depend on
`titan_test` as a dev-dependency, and replace `JumpGamePlugin` and the assertions
with your game's plugin and observable state. In a sibling workspace crate:

```toml
[dev-dependencies]
titan_test = { path = "../titan_test" }
```

Use your usual Bevy imports for `App`, input types, components, and schedules;
import `Sim` from `titan_test`. Keep gameplay plugins separate from rendering,
window creation, audio devices, and asset-dependent presentation. Do **not** add
`DefaultPlugins` to `Sim::new`: the headless plugins are already installed.

## What is installed?

`Sim::new(|app| { /* game setup */ })` installs precisely:

- `bevy_app::TaskPoolPlugin`
- `bevy_diagnostic::FrameCountPlugin`
- `bevy_time::TimePlugin`
- `bevy_app::ScheduleRunnerPlugin::run_once()`
- `bevy_input::InputPlugin`
- `bevy_transform::TransformPlugin`
- `bevy_state::app::StatesPlugin`

The first four are the essential headless `MinimalPlugins` set; there is no
window, renderer, winit event loop, asset plugin, or full `DefaultPlugins` group.
Ticks call `App::update`, not the schedule runner's continuous run loop. State
transitions and parent/child transform propagation still work.

`Sim::from_app(app)` wraps an existing app instead of installing this plugin
set. Install the plugins your systems need before handing it over. The wrapper
requires `TimePlugin` and synchronously ready plugins. Input helpers additionally
require `InputPlugin` with support for the input devices used. The wrapper finishes and cleans up plugins
if needed; construction does not run `Startup` or increment the completed tick
count. Both constructors replace frame timing **and** the fixed timestep with
60 Hz after setup, even if your game configured another rate. Choose your test
rate with `with_fixed_dt` and, if needed, set `Time<Fixed>` independently afterward.

## Time, input, and inspection

- `with_fixed_dt(seconds)` sets **both** frame duration and `Time<Fixed>`'s
  timestep in the main app and every sub-app with `TimePlugin`; clockless
  sub-apps are left unchanged. Each real clock is primed without secretly
  running an extra app update, so even a sub-app's first tick advances the full
  duration. Virtual time speed and pause settings are preserved independently
  for each world. The virtual delta cap is raised to accommodate the configured
  duration **after time scaling**, including when a paused clock resumes.
- The default frame and fixed timestep are both `1.0 / 60.0` seconds.
- `current_tick()` counts **completed** updates, starting at zero. `tick()`
  advances once; `run_ticks(n)` advances exactly `n` times.
- `press(KeyCode::Space)` / `release(KeyCode::Space)` queue transitions for the
  next update. Mouse buttons work the same way. Press holds until released.
- `tap(button)` queues a press and **advances one tick**, then queues release for
  the next tick. Do not add a second tick expecting it to be the press frame.
  For multiple buttons on one frame, queue multiple `press` calls, tick once,
  then queue their releases rather than calling `tap` repeatedly.
- Input travels through Bevy's `KeyboardInput` and `MouseButtonInput` messages
  and `InputPlugin`, so game systems see real `ButtonInput` pressed,
  just-pressed, and just-released transitions, not manually patched resources.
  Their `window` is a spawned empty entity, not a `Window` component or an invalid
  placeholder; no window crate is required. This entity is visible to whole-world
  queries, counts, and cleanup systems. Systems that query window components
  need custom input messages. Keyboard messages have an unidentified logical key,
  no text, and `repeat = false`: use `ButtonInput<KeyCode>`, not `ButtonInput<Key>`.
- `connect_gamepad()` returns a `GamepadSlot` in the lowest unused slot and
  spawns a virtual controller. Queue `press(pad.button(GamepadButton::South))`,
  `release(...)`, or `tap(...)` just like keyboard/mouse buttons. A raw value of
  1.0 presses and 0.0 releases. No hardware/gilrs plugin is needed.
- `set_axis(pad, GamepadAxis::LeftStickX, 0.75)` queues a raw value in
  `-1.0..=1.0`; `set_button_value(pad, GamepadButton::LeftTrigger2, 0.8)` queues
  an analog button value in `0.0..=1.0`. Values persist until changed. Both
  reject nonfinite or out-of-range values before queuing input.
- Connections use `GamepadConnectionEvent` and `RawGamepadEvent::Connection`;
  buttons and axes use `RawGamepadEvent::{Button, Axis}`. Bevy applies
  `GamepadSettings`, including dead zones, change thresholds, and digital button
  thresholds. In Bevy 0.20, `ButtonInput<GamepadButton>` lives inside each
  `Gamepad`: read `pad.digital()`, not a global resource. `Gamepad::get` exposes
  accepted raw values; `GamepadAxisChangedEvent` carries scaled, dead-zone-aware
  values. The harness does not change these upstream semantics.
- `gamepad_entity(pad)` resolves a connected slot to its entity. The entity
  exists immediately after `connect_gamepad`, but Bevy adds `Gamepad` on the next
  update, before processing its queued input. Use the entity to customize
  `GamepadSettings` or query `Gamepad`. Virtual entities remain visible to
  whole-world queries; do not despawn them during playback.
- `mouse_motion(Vec2::new(12.0, -3.0))` sends `MouseMotion` on the next update.
  Bevy adds all that frame's deltas into `AccumulatedMouseMotion` and resets it
  on the following update. Both delta components must be finite.
- `world()` / `world_mut()` expose the world; `resource::<R>()` reads a resource.
  `single::<&Player, With<Controlled>>()` asserts exactly one matching entity;
  `query::<&Player, ()>()` collects all matching read-only items. Both query
  helpers need `&mut Sim` to prepare query state.
- `run_until(max_ticks, condition)` checks the **initial** world, then after each
  tick. It returns ticks advanced by that call (zero for an already-true
  condition), and fails if the condition is still false at the budget.
  `run_until_named(max_ticks, "player lands", condition)` adds a useful label to
  the timeout. Conditions receive `&World`, so inspect resources, entities, or
  known entity IDs without mutating the simulation.

One-frame input edges need care if your own app changes the relationship between
frame updates and fixed updates: zero or multiple fixed steps can miss or repeat
an edge. The jumping game deliberately uses one fixed step per simulation tick.

### A virtual controller and mouse look

```rust
use bevy_input::{gamepad::{Gamepad, GamepadAxis, GamepadButton}, mouse::AccumulatedMouseMotion};
use bevy_math::Vec2;
use titan_test::Sim;

let mut sim = Sim::new(|_| {});
let pad = sim.connect_gamepad();
sim.press(pad.button(GamepadButton::South));
sim.set_axis(pad, GamepadAxis::LeftStickX, 0.75);
sim.mouse_motion(Vec2::new(12.0, -3.0));
sim.tick();
let gamepad = sim.world().get::<Gamepad>(sim.gamepad_entity(pad).unwrap()).unwrap();
assert!(gamepad.digital().just_pressed(GamepadButton::South));
assert_eq!(gamepad.get(GamepadAxis::LeftStickX), Some(0.75));
assert_eq!(sim.resource::<AccumulatedMouseMotion>().delta, Vec2::new(12.0, -3.0));
```

[`tests/analog_input.rs`](tests/analog_input.rs) exercises a game system reading
these inputs, independent controllers, filtering, and helper/script equivalence.

## Write and play back a RON input script

```ron
(
    version: 1,
    events: [
        (tick: 10, action: Press(Key(Space))),
        (tick: 11, action: Release(Key(Space))),
        (tick: 30, action: Tap(Mouse(Left))),
    ],
)
```

```rust,ignore
use titan_test::InputScript;

let script = InputScript::from_ron(include_str!("inputs/jump.ron"))?;
sim.run_script(&script, 31);
assert_eq!(sim.current_tick(), 31);
// Tick 30's mouse press ran; its release is pending for tick 31.
sim.run_script(&script, 60); // Continue the same simulation, not a fresh replay.
let saved = script.to_ron()?;
```

The current format is **version 2** (`SCRIPT_VERSION` and the default script
version). **Version 1 remains supported unchanged** for keyboard/mouse buttons,
including the example above. Parsing returns a RON error on invalid syntax or
unknown fields. `InputScript::validate()` checks compatibility and every event
without playing the script; use it instead of comparing against `SCRIPT_VERSION`.
Playback calls this same validation before advancing time: unsupported
versions, version 2 actions in a version 1 script, and invalid analog values or
mouse deltas panic rather than partially replaying a recording.

Version 2 adds gamepad slots, analog values, and raw mouse motion:

```ron
(
    version: 2,
    events: [
        (tick: 0, action: ConnectGamepad(slot: 0)),
        (tick: 0, action: Press(Gamepad(slot: 0, button: South))),
        (tick: 0, action: SetAxis(slot: 0, axis: LeftStickX, value: 0.75)),
        (tick: 0, action: MouseMotion(x: 12.0, y: -3.0)),
        (tick: 2, action: SetButtonValue(slot: 1, button: LeftTrigger2, value: 0.8)),
        (tick: 3, action: Tap(Gamepad(slot: 1, button: South))),
        (tick: 4, action: Release(Gamepad(slot: 0, button: South))),
        (tick: 4, action: SetAxis(slot: 0, axis: LeftStickX, value: 0.0)),
    ],
)
```

Slots are **stable u32 numbers**, not serialized Bevy entities. A slot connects
on its first button/axis action; explicit `ConnectGamepad` is optional and useful
for a neutral controller. Connecting an existing slot is a no-op, not a reset.
Slots can be sparse (using slot 7 does not create slots 0 through 6), and remain
mapped across split playback and helper calls. `GamepadSlot(7)` in Rust refers to
that same slot. Each `Sim` has its own mapping; a fresh simulation recreates it.
Multiple mouse deltas on one tick add up; unlike held axes/buttons, they do not
persist. Disconnect/reconnect and rumble are not part of this format.

The public `InputScript { version, events }`, `ScriptEvent { tick, action }`,
`InputAction`, `InputButton`, and `GamepadSlot` types also allow constructing
scripts directly in Rust. Float-valued actions/scripts implement `PartialEq`,
not `Eq`.

Ticks are **absolute, zero-based indices**: event tick `N` is injected before
update `N`, when `current_tick() == N`. `run_script(&script, until_tick)` uses an
**exclusive endpoint**. Starting at zero and running to 31 executes updates
0 through 30. A scripted `Tap` does not add an extra update; it releases before
update `N + 1`. A release pending at an endpoint remains queued for continuation,
including continuation with ordinary `tick()`.

On continuation, events earlier than `current_tick()` are ignored; input state
is not reconstructed from missed events. Events need not be sorted, and events
at the same tick execute in file order. To replay from the beginning, create a
fresh `Sim`. Keep the timestep identical when comparing recordings: script time
is measured in simulation ticks, not wall-clock seconds.

## Golden-file world snapshots

Enable the optional `snapshots` feature in your dev-dependency:

```toml
[dev-dependencies]
titan_test = { path = "../titan_test", features = ["snapshots"] }
bevy_reflect = { path = "../bevy_reflect" }
```

No snapshot/JSON dependency is added by this feature when it is disabled.
Golden files are ordinary, pretty JSON from `titan_snapshot`, not restorable
worlds. Put them alongside your integration tests, for example
`tests/snapshots/player_after_120_ticks.json`. Paths are explicit rather than
inferred from the caller's source file; use `CARGO_MANIFEST_DIR` so assertions
don't depend on where you launch Cargo.

This complete pattern belongs in an integration test (replace the setup with
your gameplay plugin):

```rust,no_run
# #[cfg(feature = "snapshots")]
# {
use bevy_ecs::prelude::*;
use bevy_reflect::{Reflect, TypePath};
use titan_test::{
    Sim, SnapshotAssertConfig,
    titan_snapshot::{DiffConfig, EntityMatching, SnapshotConfig, TypeFilter},
};

#[derive(Component, Reflect)]
#[reflect(Component)]
struct Health {
    value: f32,
}

let mut sim = Sim::new(|app| {
    app.register_type::<Health>();
    app.world_mut().spawn((Name::new("Player"), Health { value: 100.0 }));
});
sim.run_ticks(120);

let config = SnapshotAssertConfig::new(SnapshotConfig {
    components: TypeFilter::only([Health::type_path().into()]),
    resources: TypeFilter::only([]), // Leave out unrelated resources.
    ..Default::default()
})
.with_diff(DiffConfig { float_tolerance: 0.0001 })
.with_entity_matching(EntityMatching::ByName)
.with_entity_filter(|entity| entity.components.contains_key(Health::type_path()));

sim.assert_snapshot(
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/player_after_120_ticks.json"),
    &config,
);
# }
```

1. Run your test normally. A **missing golden fails**, with the test/thread
   name, completed tick number, path, and instructions. Nothing is written.
2. Explicitly generate it:
   `TITAN_UPDATE_SNAPSHOTS=1 cargo test -p your_game player_after_120_ticks`.
   Only the exact value `1` enables updates. This creates parent directories
   and writes or overwrites each golden reached by the selected test.
3. Review the JSON and commit it with the test. Unset the variable, rerun the
   test, and keep it **unset in CI**. Update mode writes instead of comparing;
   a passing update run is not regression verification.
4. On a gameplay regression, the assertion reports a readable matched
   `WorldDiff`, including component/field names and old/new values, for example
   `value: 100.0 -> 90.0`. Diff output is capped at 80 lines / 8 KiB with a
   truncation notice. In comparison mode, invalid JSON and I/O errors fail,
   never auto-regenerate. Explicit update mode can replace malformed JSON.

Choose entity identity deliberately. `SnapshotAssertConfig::default()` uses
exact ID matching, which is fragile for saved files from earlier runs: even one
earlier spawn can shift IDs. Prefer `ByName` with unique, stable names, or
`ByComponent(StableId::type_path().into())` with a registered, captured,
game-owned stable key (the whole serialized component, including struct keys).
Missing, opaque, and duplicate keys fail explicitly; there is no ID fallback.
Typed entity references to selected, uniquely keyed entities compare by key.

**Type filters do not filter entities.** Captures retain even empty entities,
including the harness's input entity. Use `with_entity_filter` to select only
gameplay entities, as above; unrelated earlier spawns then do not break name/key
comparisons. Resources are selected separately with `SnapshotConfig::resources`.
The entity predicate runs after component filtering, so select using a retained
component or name metadata. Golden files are not re-filtered when loaded: after
changing capture or entity selection, intentionally regenerate them. References
to excluded/unkeyed entities remain raw IDs and may differ across runs.

Register observable types using `#[reflect(Component)]` or
`#[reflect(Resource)]` and `app.register_type::<T>()`. Opaque values only expose
presence and an opacity reason: internal changes and reflection-ignored fields
cannot fail an assertion. Float tolerance is absolute and inclusive; integers
and entity keys compare exactly. See
[`titan_snapshot`'s README](../titan_snapshot/README.md) for full filtering,
identity, entity-reference, serialization, and determinism limitations.
[`tests/snapshots.rs`](tests/snapshots.rs) tests the workflow without mutating the
parallel test runner's environment.

## Determinism: what you must control

`with_seed(42)` inserts `SimSeed(42)` as a **resource**, available to game systems
including `Startup`, but not plugin build/finish/cleanup hooks that already ran
before `with_seed`. It does not seed a global RNG, intercept random calls, or
replace randomness in third-party plugins. Read `Res<SimSeed>` and initialize
your game's own deterministic RNG or seed-derived state explicitly. The replay
regression in [`tests/script.rs`](tests/script.rs) derives both a resource and a
component from that seed, plays the same input script twice, and compares them.
Replacing `SimSeed` after startup does not automatically reseed an RNG.

For repeatable runs, control all relevant inputs:

- Use the same timestep, seed, plugin configuration, starting world, and script.
- Schedules use `ExecutorKind::SingleThreaded` **by default**, even when the game
  enables multithreading. Import `ExecutorKind` from `titan_test`; opt out with
  `with_executor_kind(ExecutorKind::MultiThreaded)`. Actual concurrency depends on
  Bevy/task-pool feature flags. Still explicitly order systems whose behavior
  depends on ordering; sequential execution is not a substitute for dependencies.
  Parallel iteration or task-pool work inside a system is not serialized.
  The harness replaces custom executors and their local settings. It configures
  existing schedules and new ones discovered before each tick; schedules created
  and immediately run, or replaced under an existing label, by a system must
  configure their own executor. Calling `world_mut()` invalidates the policy
  cache so schedule replacements made by the test are picked up.
- Do not depend on real elapsed time, OS input, filesystem/network timing,
  nondeterministic entity/query iteration, or unordered collections.
- Async work may not finish on the same tick across runs. Arrange deterministic
  completion or replace external services in tests.
- Floating-point simulation and external physics libraries may differ across
  architectures, builds, or library versions. Use tolerances for physical
  quantities and do not assume portable bit-identical results.

This harness controls time and injected keyboard, mouse, and gamepad input, not every
source of nondeterminism. Passing a replay test verifies the state you actually
assert, not all possible game behavior.
