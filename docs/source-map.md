# Source and crate map

Use this map to choose the smallest package and test surface for a change. The
root workspace contains the engine and reusable host crates. The starter and
arena are independent packages with their own lockfiles, so validate them with
their manifest paths when a change crosses that boundary.

| Package or directory | Responsibility | Main entry point | Representative tests |
| --- | --- | --- | --- |
| [`titan`](../src/lib.rs) | ECS storage, schedules, input and replay, software rendering, UI, and transport-neutral inspection of an `App` | [`src/lib.rs`](../src/lib.rs) | [`tests/ecs_reference_model.rs`](../tests/ecs_reference_model.rs), inspection tests in [`src/inspection.rs`](../src/inspection.rs) |
| [`titan-cli`](../crates/titan-cli/src/main.rs) | Command parsing, local workflows, runtime discovery, inspection requests, and human or JSON output | [`crates/titan-cli/src/main.rs`](../crates/titan-cli/src/main.rs) | [`crates/titan-cli/tests/command_contracts.rs`](../crates/titan-cli/tests/command_contracts.rs), [`json_output.rs`](../crates/titan-cli/tests/json_output.rs) |
| [`titan-protocol`](../crates/titan-protocol/src/lib.rs) | Transport-neutral request, response, entity, command, query, and capture wire types | [`crates/titan-protocol/src/lib.rs`](../crates/titan-protocol/src/lib.rs) | wire-shape and response-correlation tests in [`lib.rs`](../crates/titan-protocol/src/lib.rs) |
| [`titan-remote`](../crates/titan-remote/src/lib.rs) | Authenticated loopback discovery, bounded HTTP transport, and the runtime request queue | [`crates/titan-remote/src/lib.rs`](../crates/titan-remote/src/lib.rs) | discovery, authentication, timeout, and queue tests in [`lib.rs`](../crates/titan-remote/src/lib.rs) |
| [`titan-diagnostics`](../crates/titan-diagnostics/src/lib.rs) | Bounded diagnostic bundles plus deterministic capture comparison and history | [`crates/titan-diagnostics/src/lib.rs`](../crates/titan-diagnostics/src/lib.rs) | [`crates/titan-diagnostics/tests/inspector.rs`](../crates/titan-diagnostics/tests/inspector.rs), [`comparison_report.rs`](../crates/titan-diagnostics/tests/comparison_report.rs) |
| [`titan-macros`](../crates/titan-macros/src/lib.rs) | `Component` and `Inspect` derive macros | [`crates/titan-macros/src/lib.rs`](../crates/titan-macros/src/lib.rs) | [`tests/component_derive.rs`](../tests/component_derive.rs), [`inspection_derive_compile.rs`](../tests/inspection_derive_compile.rs) |
| [`titan-browser`](../crates/titan-browser/src/lib.rs) | WASM host for the procedural RPG and browser inspection protocol | [`crates/titan-browser/src/lib.rs`](../crates/titan-browser/src/lib.rs) | browser-session and player tests in [`lib.rs`](../crates/titan-browser/src/lib.rs) |
| [`titan-render-wgpu`](../crates/titan-render-wgpu/src/lib.rs) | Native and browser GPU surfaces, sprite composition, capture, and bounded 3D rendering | [`crates/titan-render-wgpu/src/lib.rs`](../crates/titan-render-wgpu/src/lib.rs) | [`crates/titan-render-wgpu/tests/offscreen.rs`](../crates/titan-render-wgpu/tests/offscreen.rs), [`three_d.rs`](../crates/titan-render-wgpu/tests/three_d.rs) |
| [`starters/minimal`](../starters/minimal/src/lib.rs) | Copyable standalone game host with native, browser, inspection, diagnostics, and rendering wiring | [`starters/minimal/src/lib.rs`](../starters/minimal/src/lib.rs) | host tests in [`src/bin/play.rs`](../starters/minimal/src/bin/play.rs), [`scripts/test-browser.mjs`](../starters/minimal/scripts/test-browser.mjs) |
| [`games/arena`](../games/arena/src/lib.rs) | Standalone survival game exercising ECS gameplay, native/browser hosts, inspection, save/load, and replay | [`games/arena/src/lib.rs`](../games/arena/src/lib.rs) | gameplay tests in [`src/game.rs`](../games/arena/src/game.rs), inspection tests in [`src/live.rs`](../games/arena/src/live.rs) |

## Native CLI inspection route

For a concrete example, `titan --instance demo entities` reaches game state
through these boundaries:

1. [`titan-cli` parses the command](../crates/titan-cli/src/main.rs) and
   [`dispatch::classify`](../crates/titan-cli/src/dispatch.rs) converts it to a
   typed `titan_protocol::Request::Entities`.
2. [`remote::execute_remote`](../crates/titan-cli/src/remote.rs) discovers and
   selects the registered runtime, wraps the request in a `RequestEnvelope`,
   and calls `titan_remote::send`.
3. [`titan-remote`](../crates/titan-remote/src/lib.rs) authenticates the bounded
   loopback HTTP request and places it on `RequestQueue`; the transport thread
   does not access game state.
4. A host such as the [minimal starter](../starters/minimal/src/main.rs) drains
   that queue at a runtime safe point and passes the envelope to
   `Inspector::dispatch`.
5. [`Inspector`](../src/inspection.rs) validates the envelope and executes the
   entity query against `App` and `World`, returning a correlated
   `ResponseEnvelope` through the waiting reply handle.
6. `titan-remote::send` validates response correlation, then the CLI renders
   the response in the selected human or JSON format.

The browser route replaces loopback discovery and `titan-remote` with
[`titan-browser`](../crates/titan-browser/src/lib.rs), while keeping the same
protocol envelopes and `Inspector` boundary.
