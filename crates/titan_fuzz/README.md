# `titan_fuzz`: find a gameplay bug, keep a tiny replay

`titan_fuzz` generates game-defined actions or keyboard/mouse input scripts for a headless
[`titan_test::Sim`](../titan_test/README.md), checks gameplay invariants after
every tick, and shrinks a failure into a short reproduction. No window,
renderer, GPU, wall-clock waiting, live game, or MCP server is needed. Bevy's
existing APIs and crate names are unchanged.

## Run the deliberately broken game

From the Titan workspace:

```sh
cargo run -p titan_fuzz --example airborne_collision
cargo test -p titan_fuzz
```

[`examples/airborne_collision.rs`](examples/airborne_collision.rs) is a complete
small game. Holding D walks right; Space jumps for six ticks. Its deliberate
bug disables wall collision while airborne. The fuzzer finds a combination that
crosses the wall, prints the report with its reduced RON script, then confirms
the failure with plain `Sim::run_script`. The example exits successfully when
it finds the **expected** failure. Remove the airborne collision guard to fix
it; the example will then complain that its demonstration bug was not found.

## A runnable quick start

In a sibling workspace crate, add the harness as a dev-dependency:

```toml
[dev-dependencies]
titan_fuzz = { path = "../titan_fuzz" }
titan_test = { path = "../titan_test" }
```

Use your normal Bevy dependencies for components, input types, and schedules.
Here is a complete test using a tiny health mechanic and a small CI budget:

```rust
use bevy_app::Update;
use bevy_ecs::prelude::*;
use bevy_input::{ButtonInput, keyboard::KeyCode};
use titan_fuzz::Fuzz;
use titan_test::Sim;

#[derive(Resource)]
struct Health(u8);

fn heal(keys: Res<ButtonInput<KeyCode>>, mut health: ResMut<Health>) {
    if keys.just_pressed(KeyCode::Space) {
        health.0 = health.0.saturating_add(1).min(10);
    }
}

fn game() -> Sim {
    Sim::new(|app| {
        app.insert_resource(Health(5)).add_systems(Update, heal);
    }).with_seed(7)
}

// Add #[test] when placing this function in tests/gameplay.rs.
fn health_stays_bounded() {
    Fuzz::new(game)
        .buttons([KeyCode::Space])
        .invariant("health <= max", |world| {
            let health = world.resource::<Health>().0;
            if health <= 10 {
                Ok(())
            } else {
                Err(format!("health {health} exceeds maximum 10"))
            }
        })
        .no_nan_transforms()
        .cases(4)
        .ticks(32)
        .seed(1)
        .max_shrink_runs(50)
        .test_name("health_stays_bounded")
        .run()
        .assert_ok();
}
# health_stays_bounded();
```

The factory must build a **fresh** simulation for every case and every shrink
attempt. Keep gameplay separate from presentation: do not install
`DefaultPlugins` in `Sim::new`. The headless base plugins, input injection, and
controlled frame/fixed timestep are already installed by `titan_test`.

## Fuzz the game's action layer

Use `ActionFuzz` when the game already translates input into gameplay actions.
The generator receives a portable `ActionRng`, and an adapter writes one tick's
complete action value into the world **before** `Sim::tick`. Both callbacks are
ordinary functions or closures; no action-layer plugin or trait is required.
Actions need `Clone + Serialize + DeserializeOwned`, not `Resource`, `Default`,
`Debug`, or `PartialEq`. Add `serde` with its `derive` feature to your test crate.

```rust
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use titan_fuzz::{ActionFuzz, ActionScript};
use titan_test::Sim;

#[derive(Clone, Serialize, Deserialize)]
struct Actions { movement: i32, jump: bool }

#[derive(Resource, Default)]
struct Player(i32);

fn game() -> Sim {
    Sim::new(|app| { app.init_resource::<Player>(); })
}

fn apply_actions(world: &mut World, action: &Actions) {
    // A deliberately broken handler: jumping bypasses wall collision.
    let mut player = world.resource_mut::<Player>();
    player.0 = (player.0 + action.movement).min(if action.jump { 100 } else { 2 });
}

let report = ActionFuzz::new(
    game,
    Actions { movement: 0, jump: false }, // neutral tick, used by shrinking
    |rng| Actions { movement: rng.below(5) as i32, jump: rng.below(2) == 0 },
    apply_actions,
)
.invariant("outside wall", |world| {
    if world.resource::<Player>().0 <= 2 { Ok(()) }
    else { Err("player inside wall".into()) }
})
.simplify_action(|action| vec![Actions { movement: 3, jump: action.jump }])
.cases(4).ticks(32).seed(68).max_shrink_runs(100)
.run();

// Turn the standalone RON into an ordinary regression, without a fuzzer.
if let titan_fuzz::FuzzReport::Failed(failure) = report {
    let ron = failure.script.to_ron().unwrap();
    let script = ActionScript::<Actions>::from_ron(&ron).unwrap();
    let mut sim = game();
    script.replay(&mut sim, apply_actions);
    assert!(sim.resource::<Player>().0 > 2); // confirms this demonstration bug
}
```

The explicit neutral action is saved with the regression. Missing ticks apply
that value, **not** the preceding event: adapters must overwrite held movement
and one-shot aim on every tick. This makes dropping an action genuinely remove
its effect rather than accidentally extending a hold. Ensure the action
resource exists in the factory; tick 0's adapter runs before `Startup`. Do not
let a human-input system overwrite the injected actions in a headless test.

`ActionScript` saves a version, exclusive tick endpoint, neutral value and
strictly ordered `(tick, action)` events. Parsing rejects unsupported versions,
duplicate/unsorted ticks and events beyond the endpoint. `replay` can resume a
partially played simulation and propagates ordinary game panics. To assert a
transient invariant in a regression, replay prefixes one tick at a time and
check after each update; the final saved endpoint reproduces the finding.

Action campaigns use the same `Failure` / `FuzzReport` types (with script and
configuration type parameters), invariant checks, panic handling, output paths
and `assert_ok` / `save_script` behavior as button campaigns. Sequence shrinking
truncates after the failure, drops chunks/ticks, then moves remaining actions
earlier without stacking values on a tick. Every accepted candidate is
confirmed on a fresh simulation; an inconsistent confirmation returns `Flaky`
with the original script. An optional `.simplify_action(...)` supplies a finite,
ordered list of smaller values per surviving event; the first confirmed value
is accepted. This hook runs once per event, so provide your smallest useful
candidate first. The shrink budget includes confirmation runs. Non-string
panics preserve the original sequence without shrinking.

Generation and simplification callback panics propagate as configuration
errors. Factory, adapter, update and invariant panics become `no_panics` findings
under unwind; aborts and hangs cannot be caught. Use a fresh factory and pure,
deterministic callbacks; keep the adapter, generator and game seed with the
test because Rust closures are not serialized in the report.

[`demos/doom/tests/fuzz_actions.rs`](../../demos/doom/tests/fuzz_actions.rs)
runs eight headless 600-tick campaigns against Doom's public gameplay APIs.
It generates movement, aim, fire, interaction and occasional restart actions,
checks walls and closed doors, and verifies tick advancement while playing
(with restarts resetting the gameplay clock and terminal phases freezing it).
It uses a serializable DTO to avoid imposing serde on the game's resource.
The demo depends on `titan_fuzz` for tests, not the other way around; no Doom
gameplay source is modified.
[`tests/actions.rs`](tests/actions.rs) finds a deliberately broken handler,
shrinks it to a single readable action, saves it and replays the same failure.

```sh
cargo test -p titan_doom --no-default-features --test fuzz_actions
cargo test -p titan_fuzz --test actions
```

## Choose relevant inputs and bounded work

- `.buttons(...)` selects the alphabet. There is no default input alphabet:
  explicitly choose the keyboard keys and/or mouse buttons your mechanic uses.
  For mixed types, supply an array of `titan_test::InputButton` values.
- `.cases(n)` and `.ticks(n)` bound the campaign. Start with a few cases and a
  few dozen ticks in CI. Increase both for a local search rather than making
  ordinary tests unexpectedly expensive.
- `.seed(n)` controls **input generation**, not the game's own randomness.
  Use the same factory setup, timestep, game seed, and fuzz configuration to
  reproduce a campaign.
- `.generator(Generator { event_density, min_hold_ticks, max_hold_ticks })`
  tunes press/release frequency and button hold lengths. `Generator::default()`
  is a useful starting point. Hold lengths are in simulation ticks, not seconds.
- `.max_shrink_runs(n)` bounds extra candidate executions during minimization.
  A zero budget still reports the finding, without spending candidate runs.
- `.test_name("name")` gives findings a stable directory name; explicitly name
  tests so unrelated regressions do not overwrite one another.
- `.output_dir(path)` changes the default `target/titan_fuzz` output root.

A failed case is replayed in a fresh simulation before shrinking. That replay
is additional work, separate from the shrink-candidate budget. Every replay is
bounded by the original case's tick limit. Limits bound updates and replay
counts, not elapsed time: a factory, game system, or invariant that hangs cannot
be interrupted in-process.

Generation uses a fixed, small `SplitMix64` RNG with wrapping `u64` arithmetic,
a mixed per-case seed, 53-bit probability draws, and bounded multiply-high hold
sampling. Its algorithm is pinned by a known-vector test, so dependency updates
or target word size do not change the corpus. Hold sampling has negligible
rounding bias; this is a simulation RNG, not a cryptographic one. No OS entropy
or global RNG is used.

`Generator::generate(&buttons, ticks, seed, case_index)` can regenerate a case
independently of earlier cases. Reports retain the seed, zero-based case index,
and `FuzzConfig`, including the generator settings and button order. For example,
using a `Failure` named `failure`:

```rust,ignore
let original = failure.config.generator.generate(
    &failure.config.buttons,
    failure.original_ticks,
    failure.seed,
    failure.case_index,
);
```

## Write invariants that explain what broke

An invariant receives `&World` and returns `Result<(), String>`; its name
identifies the rule, while the error describes the particular violation. Return
state values and expected bounds rather than just `"bad player"`. Examples:

- health is between zero and maximum health;
- a player stays inside the level or outside solid walls;
- inventory quantities never become negative;
- a locked door cannot be crossed without its key;
- a grounded character has a plausible vertical velocity.

Checks run **after every simulation tick**, starting with tick zero after
`Startup` has run. Startup-created components and resources are therefore
available to invariants. A transient violation is a failure even if later ticks
would repair it. There is no extra warm-up frame before script tick zero.

Prefer errors to panics inside an invariant, and keep checks side-effect free.
Do not consume randomness or mutate shared state in them. Choose a named rule
that is true at every observed tick, not an eventual condition like "the player
will land" without a time bound. When inspecting a whole world, filter for your
game components: the simulation also contains internal entities.

`.no_nan_transforms()` checks **all** `Transform` translation, rotation, and
scale fields for non-finite values, including infinity as well as NaN. Its
built-in name is `no_nan_transforms`. Game-system panics are caught and reported
under the reserved name `no_panics`, with the panic message. A panic-abort build
or process abort cannot be recovered by `catch_unwind`. Non-string
`panic_any` payloads have no general-purpose value comparison, so the fuzzer
reports them without shrinking and says so in the violation message. A payload
type change on the original replay is `Flaky`; differing values of the same
opaque type cannot be detected. Prefer descriptive string panics or invariant
errors when you want a shrunk reproduction.

## How shrinking works

A reproducible failure is minimized while preserving the **same invariant**:

1. Cut off ticks after the failure.
2. Remove chunks of events, then individual events.
3. Try shorter button holds.
4. Try moving events earlier.

Each accepted candidate is checked and confirmed against fresh simulations;
both runs count towards the budget. A confirmation must agree on the invariant,
failing tick, and message. For panic failures, candidates must also preserve the
original panic message, not merely fail with some other panic. Shrinking stops at
its configured execution budget and returns the best reproduction found so far;
this is not a guarantee of a globally smallest script. Removed events can leave
buttons held, so the minimal script need not retain every original press/release
pair. The final run length is part of the reproduction too.

`FuzzReport` is one of:

- `Passed { cases, ticks }`: all checked cases passed (`ticks` is ticks per case);
- `Failed(Failure)`: the failure reproduced and was eligible for shrinking;
- `Flaky(Failure)`: either the original replay or a candidate confirmation
  disagreed. Shrinking stops and reports the original script and provenance,
  not a misleading "minimal" regression.

`Failure` includes the invariant/message, zero-based failing tick, reduced
script and run length, original tick/event counts, seed/case/config, shrink
execution count, and output path. Reports implement `Debug`, `PartialEq`,
`Serialize`, and `Display`. `Display` includes inline RON and a replay snippet.
`.run()` returns the report; `.assert_ok()` saves a failed or flaky script and
then panics. With `.test_name("health_stays_bounded")`, the default destination
is beneath `target/titan_fuzz/health_stays_bounded/`, using a filename derived
from the invariant name.

## Turn a finding into a regression test

After `assert_ok()` saves a finding, copy its `.ron` file into your committed
test fixtures. Keep the reported **reduced tick length** alongside it: scripts
contain input events but do not encode the total playback duration. Replace the
fixture path and tick count below with the values in the report:

```rust,ignore
use titan_test::InputScript;

#[test]
fn airborne_collision_regression() {
    let script = InputScript::from_ron(include_str!("inputs/inside_wall.ron"))
        .expect("valid regression fixture");
    let mut sim = game(); // The same factory setup and game seed as the finding.
    sim.run_script(&script, 4); // Example only: use failure.ticks from the report.
    assert!(inside_wall(sim.world()).is_ok()); // Passes after fixing the bug.
}
```

`Sim::run_script` uses absolute, zero-based event ticks and an **exclusive**
endpoint. A failure at script tick 3 needs at least four updates to replay.
Always replay from a fresh simulation. For an invariant that can fail and later
recover, check it after each update instead of only at the endpoint:

```rust,ignore
for tick in 0..total_ticks {
    sim.run_script(&script, tick + 1);
    inside_wall(sim.world()).expect("player stays inside the wall");
}
```

This button regression depends only on `titan_test`; it does not need to
generate random inputs or shrink again. Action regressions use
`ActionScript::replay` from `titan_fuzz`, without running a campaign.

## Limits and determinism

This is property-based **input/action** testing, not coverage-guided fuzzing or
arbitrary world-state mutation. The raw `Fuzz` API supports keyboard keys and
mouse buttons, not mouse motion, gamepads, or analog axes. `ActionFuzz` supports
user-defined gameplay actions, including analog movement and aim, through the
game's own adapter; it does not provide a standard action-mapping layer.
Both modes explore the chosen inputs and starting state, so a passing campaign
is not proof of correctness or exhaustive coverage.

The generated input stream is seedable and portable. That does not make an
arbitrary game deterministic. `Sim::with_seed` supplies a `SimSeed` resource;
your game must actually use it for its own RNG. Control system ordering,
external I/O, async completion, wall-clock access, unordered iteration, and
floating-point assumptions. See the
[`titan_test` determinism guidance](../titan_test/README.md#determinism-what-you-must-control).
A successful confirmation replay is only a practical check, not a proof that
all future executions will reproduce. Treat `Flaky` as a request to investigate
nondeterminism, not as a stable shrunk regression.
