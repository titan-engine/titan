# Typed ECS systems

`App::add_systems` accepts exclusive `fn(&mut World)` functions and functions
with zero to four typed parameters: `Query`, `Res`, `ResMut`, and `Commands`.
Execution defaults to sequential registration order. Native applications can opt
into compatible-system concurrency:

```rust
use std::num::NonZeroUsize;
use titan::{App, ExecutorPolicy};
let mut app = App::new();
app.set_executor_policy(ExecutorPolicy::Parallel {
    max_threads: NonZeroUsize::new(2).unwrap(),
});
```

The limit bounds simultaneously executing callbacks. One thread and WebAssembly
use sequential execution. Parallel batches contain contiguous compatible typed
systems; conflicting accesses, exclusive systems, Commands and ApplyDeferred
separate batches. Commands barriers do not add a deferred flush. Shared reads can
run concurrently; writes to the same component/resource serialize. Captured state,
interior mutation and external effects are outside access metadata, so use
sequential execution when their order matters. See [executor design](executor.md)
for safety, error and panic semantics.

```rust
use titan::{Commands, Component, Query, Res};

#[derive(Component)]
struct Position(i32);
#[derive(Component)]
struct Velocity(i32);
struct Speed(i32);

fn movement(mut query: Query<(&mut Position, &Velocity)>, speed: Res<Speed>) {
    query.for_each(|_entity, (position, velocity)| {
        position.0 += velocity.0 * speed.0;
    });
}
```

Queries accept one component reference or tuples through arity four. The first
component determines dense traversal order. Use `for_each_sorted` for ascending
entity index/generation order. Missing component storage produces no matching
rows. Callback references cannot escape the row visit; the implementation uses
safe Rust borrowing without raw-pointer query iteration.

`Res<T>` and `ResMut<T>` require an existing resource. Shared accesses may repeat;
any mutable/shared or mutable/mutable overlap to the same component or resource
is rejected before registration. Components and resources have separate access
namespaces. Validation considers the whole system, including separate queries.
`try_add_systems` returns declaration errors; `add_systems` panics on an invalid
declaration. `system_metadata` exposes declared access in registration order.

`Commands` reserves entity IDs and queues changes without borrowing component
storage or resources. Add `ApplyDeferred` with `app.add_systems(label,
ApplyDeferred)` when a later system in the same schedule must see queued changes.
Schedules still flush at their end. A failed system or explicit deferred boundary
stops the remaining systems in that schedule; earlier changes are not rolled back.

Missing resources become `AppError::System` failures. Checked stepping stops at
the failed tick and reports its observed frame; the Inspector returns structured
`system_errors` details and leaves its successful-operation revision unchanged.
Deferred failures remain available through `take_deferred_errors`, while
`take_system_errors` drains only typed-system failures.

## Bundles

Use `World::spawn_with` or `Commands::spawn_with` to create an entity with a
component bundle. A bundle is a component, the empty tuple `()`, or a tuple of
up to twelve bundles; nested tuples let helper functions compose entity parts.

```rust
use titan::{Bundle, Commands, Component, World};

#[derive(Component)]
struct Position(i32);
#[derive(Component)]
struct Health(u32);

fn character() -> impl Bundle {
    (Position(0), Health(100))
}

let mut world = World::new();
let entity = world.spawn_with(character());
world.insert_bundle(entity, (Position(10), Health(80))).unwrap();

fn reinforcements(mut commands: Commands) {
    let entity = commands.spawn_with(character());
    commands.insert_bundle(entity, (Position(20),));
}
```

`insert_bundle` replaces components in left-to-right tuple order, discarding
replaced values. If a type repeats, the final value wins. Other components on
the entity remain unchanged. Immediate insertion checks entity liveness before
writing any components, including for an empty bundle. A deferred bundle is one
insertion command: an invalid handle produces one `DeferredOperation::Insert`
error and no component writes; later commands still run. Reserved entities and
their bundles become visible only when commands are applied.

`Bundle` is sealed. Reusable constructors should return tuples or `impl Bundle`;
a custom bundle derive is not required.

## Exclusive systems and errors

Exclusive functions are supported. Exclusive closures require an explicit
argument type so Rust can distinguish their signature from typed parameters:

```rust
# use titan::{App, FixedUpdate, World};
# let mut app = App::new();
app.add_systems(FixedUpdate, |world: &mut World| {
    // Existing direct-world system body.
    let _ = world.entity_count();
});
```

`try_advance_fixed` and `apply_deferred` return `Vec<AppError>` on failure;
match `AppError::Deferred` for deferred-command failures.
`spawn()`, `insert(entity, component)`, and `Commands::spawn_with(component)`
support individual components. Use `spawn_with((first, second))` or
`insert_bundle(entity, (first, second))` when previous component values are not
needed. A one-element tuple requires a trailing comma: `(component,)`.
Mutable joins use scoped callbacks, not iterators.

## RPG example

All RPG fixed-update systems declare typed access. Scheduled input is an existing
resource that becomes enabled on the first accepted protocol input; it otherwise
leaves interactive and direct replay input untouched. Shard collection and shrine
activation are separate systems, with structural changes visible at the schedule's
final deferred boundary. Setup and immutable render extraction keep direct world
access for clarity. Spawning uses `(Position, marker, Name)` bundles.
