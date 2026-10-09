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
require keyboard/mouse `InputPlugin`. The wrapper finishes and cleans up plugins
if needed; construction does not run `Startup` or increment the completed tick
count. Both constructors replace frame timing **and** the fixed timestep with
60 Hz after setup, even if your game configured another rate. Choose your test
rate with `with_fixed_dt` and, if needed, set `Time<Fixed>` independently afterward.

## Time, input, and inspection

- `with_fixed_dt(seconds)` sets **both** frame duration and `Time<Fixed>`'s
  timestep. The first tick advances the full duration: the real clock is primed
  without secretly running an extra app update. Large requested durations are
  not lost to virtual time's default maximum-delta clamp.
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
  placeholder; no window crate is required. Systems that query window components
  need custom input messages. Keyboard messages have an unidentified logical key,
  no text, and `repeat = false`: use `ButtonInput<KeyCode>`, not `ButtonInput<Key>`.
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

The currently supported version is **1**. Parsing returns a RON error on invalid
syntax or unknown fields; playback rejects unsupported versions. The public
`InputScript { version, events }`, `ScriptEvent { tick, action }`,
`InputAction::{Press, Release, Tap}`, and `InputButton::{Key, Mouse}` types also
allow constructing scripts directly in Rust.

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

This harness controls time and injected keyboard/mouse transitions, not every
source of nondeterminism. Passing a replay test verifies the state you actually
assert, not all possible game behavior.
