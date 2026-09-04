# Open design questions

These are intentionally unresolved. They should be answered through focused
design work or small prototypes before their answers become expensive to
change.

## First game slice

- What is the smallest RPG interaction that tests meaningful state and systems:
  combat, an inventory pickup, dialogue, a quest, or something else?
- What visual feature set is needed for the initial generated pixel art?
- How large and structurally varied must the procedural world be?
- What objective evidence demonstrates that an agent made the art "prettier"?

## Execution and browser architecture

- How much of the first milestone's browser experience is the WebAssembly game
  itself, and how much is a separate inspector application?
- How does the inspection transport behave in WebAssembly, where listening on a
  local socket is not the same as in a native process?
- Which GPU-independent renderer or reference path will produce CI captures?
- What level of pixel determinism is practical across native, WebAssembly, GPU,
  and software rendering paths?

## ECS internals

- Archetypal tables, sparse sets, or a hybrid storage strategy?
- How are entity generations and optional persistent names represented?
- What are the exact query and system-parameter APIs?
- How are access conflicts derived and validated?
- What deterministic ordering guarantees are made?
- How will the future parallel scheduler preserve or deliberately relax them?
- What state is included in snapshots, and how are opaque resources handled?

## Reflection and serialization

- Which reflection capabilities are mandatory for a derived component?
- Is serialization a separate derive/capability from inspection?
- How are custom field editors, validation, units, and ranges represented?
- How are component schemas exposed compactly enough for an agent context
  window without inventing a mandatory game manifest?

## Runtime protocol and safety

- HTTP, WebSocket, both, or a different initial transport?
- How are running games discovered and selected when several exist?
- What enables development mutation, and how is it prevented in release builds?
- At which simulation safe points are reads, mutations, and commands applied?
- How are protocol requests correlated with the precise frame they observed or
  changed?
- What authentication or origin protection is required for local and browser
  use?

## Crates and dependencies

- What is the smallest useful initial crate/workspace structure?
- Which parts should be custom immediately and which mature crates should be
  adopted for windowing, WebAssembly glue, serialization, image formats, and
  transport?
- What dependency maintenance and licensing criteria are mandatory?
- Which `wgpu` abstractions should be exposed or hidden by the first renderer?

## Quality policy

- Which Clippy lint level is enforced, beyond denying warnings?
- What are the initial build-time, test-time, and runtime performance budgets?
- Should performance assertions become part of CI, and on which stable runners
  can they be meaningful?
- What release/versioning convention best communicates frequent breaking
  revisions before a stable public release?

