# Open design questions

These are intentionally unresolved. They should be answered through focused
design work or small prototypes before their answers become expensive to
change.

## Established choices

The first slice uses a deterministic generated tile map, three shard pickups,
and shrine activation. ECS storage is sparse-set with generational entities;
execution is sequential with validated typed access and deferred structural
commands. Native inspection uses authenticated loopback HTTP/JSON and local
registration files. Browser inspection uses a WASM adapter and same-origin
message bridge. Software captures are exact references; native/browser GPU
rendering consumes the same immutable frames. See the implementation plan and
focused docs for current APIs and guarantees.

## First game slice and visual acceptance

- Which art direction should guide the required recorded visual improvement?
- What objective semantic checks and human visual review should accompany that
  before/after iteration?
- Is the current whole-map view sufficient for this slice, or should the next
  game change exercise a scrolling camera and a larger area?

## ECS and execution evolution

- When does profiling justify archetypal/hybrid storage or parallel execution?
- How will a future parallel scheduler preserve or deliberately relax canonical
  ordering guarantees?
- What state belongs in snapshots, and how should opaque resources participate?
- Should optional resource/query parameters or wider query arity come next once
  a concrete game needs them?

## Reflection and serialization

- Which reflection capabilities are mandatory for a derived component?
- Is serialization a separate derive/capability from inspection?
- How are custom field editors, validation, units, and ranges represented?
- How are component schemas exposed compactly enough for an agent context
  window without inventing a mandatory game manifest?

## Runtime protocol evolution

- Should browser hosts export diagnostic bundles automatically, and through which
  download/development-host mechanism?
- When is an outgoing browser-to-native development connection needed?
- Which workloads require interruptible or worker-isolated execution beyond
  cooperative native tick deadlines and browser frame limits?

## Crates and dependencies

- What dependency maintenance and licensing criteria are mandatory?
- Which `wgpu` abstractions should be exposed or hidden by the first renderer?

## Quality policy

- Which additional Clippy lint policies, beyond the current denied warnings,
  would catch demonstrated engine-specific defects?
- What are the initial build-time, test-time, and runtime performance budgets?
- Should performance assertions become part of CI, and on which stable runners
  can they be meaningful?
- What release/versioning convention best communicates frequent breaking
  revisions before a stable public release?

