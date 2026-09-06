# ECS-only subsystem boundary audit

An external consumer can use `World` without constructing `App`, a renderer,
an inspector, or a platform host. This is useful runtime opt-out, but
`default-features = false` does **not** make Titan an ECS-only compilation unit.
It removes PNG decoding; other core modules and their mandatory dependencies
still compile. Native host/GPU crates already have a separate dependency boundary.

This is the bounded investigation for [issue #38](https://github.com/titan-engine/titan/issues/38),
covering requirements R2.30, R2.42–44 in the [design requirements](../design-requirements.md).
The issue's original audit baseline was `c14c9dfe7239f7853eed7f0d7ddac02f4fdfc79e`;
the source and execution evidence here use `3271f2819c2a11a0e1fefa922f888e8864671800`.
No engine behavior, Cargo features, or CI jobs are changed by this investigation.

## Reproduce the historical consumer

This completed experiment is preserved at evidence-containing revision
`1c885151a2e59b5f6212939c1658f4c49408273f`. Its scripts are not maintained HEAD verification tools.
Check out the measured engine source and extract only its historical harness
from the evidence revision. Run the original commands below there;
keep generated output in that checkout's ignored `target/` directory.

```sh
git worktree add --detach /tmp/titan-subsystem-audit 3271f2819c2a11a0e1fefa922f888e8864671800
git archive 1c885151a2e59b5f6212939c1658f4c49408273f docs/subsystem-audit/reproduce.py docs/subsystem-audit/consumer.rs | tar -x -C /tmp/titan-subsystem-audit
cd /tmp/titan-subsystem-audit
```

From the repository root, with stable Rust, Python 3, Node.js, and the WASM target:

```sh
cargo fetch --locked
rustup target add wasm32-unknown-unknown
python3 docs/subsystem-audit/reproduce.py
cargo test -p titan --lib app::
```

The [runner](https://github.com/titan-engine/titan/blob/1c885151a2e59b5f6212939c1658f4c49408273f/docs/subsystem-audit/reproduce.py) copies [consumer.rs](https://github.com/titan-engine/titan/blob/1c885151a2e59b5f6212939c1658f4c49408273f/docs/subsystem-audit/consumer.rs) into a temporary
directory **outside the repository workspace**, with its own `[workspace]` and
only one dependency:

```toml
[dependencies]
titan = { path = "/absolute/path/to/titan", default-features = false }
```

It seeds resolution from the repository lockfile, resolves offline, then uses
`--locked`. The temporary manifest also forwards `titan/image-png` through its
own default feature to compare PNG-free and PNG-enabled versions of the same
consumer. That matches Titan's current default feature set; it is not a general
check for future changes to Titan's defaults. Both variants use the same lockfile.
The runner removes the temporary project on exit and keeps four dependency/feature
trees plus the resolved lockfile in `target/subsystem-audit/`. Those local trees
contain local paths; they need not be published as review evidence.

For each variant it runs native assertions, formatting/Clippy, records
`cargo tree --locked -e normal,build,features` for native and
`--target wasm32-unknown-unknown`, builds a WASM library, and executes its exported
`ecs_probe()` through Node's WebAssembly API. The WASM module must have zero imports
and return 42. No browser adapter or WASI shim is involved.

The workload checks an initially empty entity/component registry, absence of
`FixedTime` and `UiFocus` resources, derived component insertion and mutation,
and explicitly buffered despawning. It uses a direct `World` and no schedule.
Resource probes reference two public types to observe initialization, but do
not construct their subsystems. They are samples, not an exhaustive resource census.

## What is omitted, and what remains

| Boundary | Compile/dependency result for PNG-free external consumer | Runtime initialization |
| --- | --- | --- |
| ECS | Required core module, with mandatory macro support | `World::new()` creates empty allocator, component/resource maps and command queue. No framework stages. |
| App, scheduling, time | Unconditional core modules | Direct World has no clock or schedules. `App::new()` explicitly adds `FixedTime`, empty schedules/extractors and sequential policy; construction does not execute startup. |
| Input and replay | Unconditional modules; replay uses serde/JSON | Input tracking and replay data require caller construction/use; replay does not itself advance App. |
| Inspection/protocol | Core inspection and `titan-protocol`, serde and serde_json remain dependencies | Inspector construction and request handling are explicit. Linking protocol types starts no listener. |
| CPU rendering and procedural assets | Core render module, software renderer, image and geometry code remain | Rendering/image generation requires calls; World creates no renderer or images. |
| PNG decoding | `image-png` omits decoder module and the `png` dependency closure when disabled | Even when compiled, decoding needs an explicit call. |
| ECS UI | Unconditional module, coupled to render/inspection/protocol | No UI entities, focus, layout, hit testing or rendering are installed by World. UI operations are explicit. |
| Native GPU/window/remote/diagnostics hosts | `titan-render-wgpu`, wgpu, winit, pollster, ctrlc, `titan-remote`, `titan-diagnostics` are absent from the consumer graph | No host is constructed. Root native dev-dependencies used by examples do not propagate to consumers. |
| Browser host | `titan-browser`, wasm-bindgen and web-sys are absent | The consumer needs no page, bridge, DOM or GPU. Core transport-neutral `BrowserSession` code still compiles. |

The mandatory direct dependencies in both target graphs are `serde`,
`serde_json`, `titan-macros`, and `titan-protocol`. The observed PNG-free closure
also includes serde_core/serde_derive, proc-macro2, quote, syn (2 and 3),
unicode-ident, itoa, memchr and zmij. Proc macros compile for the build host,
including when the consumer targets WASM; they are not runtime WASM imports.
The PNG-enabled comparison adds png and its compression/checksum dependencies.
Neither graph includes the native or browser hosts listed above.

These are Cargo compilation/dependency facts, not claims that every compiled
function survives linker dead-code elimination. A lockfile alone does not prove
an enabled dependency: it can retain packages for disabled features. No binary-size,
compile-time, allocation, or performance threshold was measured.

## Source and runtime evidence

The constructor evidence is in [World](../../src/ecs/world.rs) (`World`'s derived
`Default` and `new`) and [App](../../src/app.rs) (`Default`, `run_schedule`,
`update` and `step_fixed`). [lib.rs](../../src/lib.rs) declares core modules
unconditionally; [Cargo.toml](../../Cargo.toml) distinguishes required, optional
PNG, and native dev-dependencies. [render/mod.rs](../../src/render/mod.rs)
contains the PNG module gate. See also the explicit operations in
[software rendering](../../src/render/software.rs), [UI](../../src/ui.rs),
[inspection](../../src/inspection.rs), and [replay](../../src/replay.rs).

Observed on 2026-09-05, macOS `aarch64-apple-darwin`, rustc 1.98.1
(`48a229cea`), Node 26.8.1:

| Verification | Result |
| --- | --- |
| Native PNG-free and PNG-enabled consumer | Both return 42 and pass initialization/component/deferred-operation assertions. |
| Both actual `wasm32-unknown-unknown` consumer builds, executed in Node | Both return 42 with zero imports; the same assertions execute in WASM. |
| External formatting and Clippy, both feature variants | Pass. |
| Native versus WASM feature trees | Same package/feature closure per variant, apart from the temporary/native path presentation. Host packages remain absent. |
| Existing App unit tests | Validate normal startup, fixed ticks, schedules, errors and extraction without changing App implementation. |

WASM support here means compilation **and execution of this ECS workload** in a
WebAssembly engine. It does not establish browser rendering/inspection support,
WASI support, native Windows support, `no_std`, or all possible ECS workloads.
The core uses `std`. WASM's executor policy yields concurrency one
([system.rs](../../src/system.rs)); native inspection wall-clock timing is
cfg-omitted on WASM while frame limits remain. Those are separate target choices,
not optional-subsystem features. The source audit and selected state assertions
support the initialization conclusions; no OS syscall/thread trace was collected.

## Boundary conclusions

At the audited revision, root `cargo check -p titan --lib --no-default-features`
checked library compilation, but did not exercise external public-API use or
execute the WASM ECS workload. The reproduction above supplies that bounded
evidence; it does not establish a permanent dependency allowlist.

The audit does not justify splitting crates or adding subsystem feature flags:
there is no observed unwanted runtime initialization or measured cost to motivate
that scope. Compile-time omission of all major core subsystems remains an unmet
boundary question under R2.44, rather than something this audit silently declares
complete. Follow-up implementation requires maintainer approval on an issue.
