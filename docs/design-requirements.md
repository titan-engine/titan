# Design requirements

This reference records Titan's product direction, requirements and unresolved
choices. Stable R1 and R2 identifiers preserve links from issues and technical
docs: R1 contains 71 entries and R2 contains 70. Their numbering comes from the
initial design discussions; no access to those discussions is needed to use this
reference. Requirements describe intent, not an implementation schedule.

**Firm** means an intended requirement or direction, not a claim that it ships.
**Preference** means a tentative direction or desired future capability.
**Open** means the choice remains unsettled. **Refined** identifies a direction
clarified by another requirement; its qualifications remain visible.
**Instruction** records a project working constraint, not an engine feature.

The [vision](vision.md) explains the durable product direction. The
[verification guide](verification.md) defines quality gates, and
[open questions](open-questions.md) tracks unresolved choices. A requirement in
this reference does not authorize implementing it immediately. This is not a
chronological decision diary; Git records changes to the design and implementation.

Implementation evidence remains separate. The [first ECS UI slice](ui.md) now
replaces the arena's direct-drawn HUD and adds an RPG quest display; it does not
claim to implement general layout or typography. The subsequent [quest
journal](journal.md) exercises explicit column placement, bounded bitmap text and
scoped keyboard focus. The RPG and arena exercise
[shared snapshot-backed interactive replay](replay.md) in native and browser
players, alongside headless verification. The arena also supports
[bounded seeking and playback speed controls](arena-replay.md); those controls
are not yet exposed by the RPG player. The [file-backed sprite exercise](assets.md) verifies shared
procedural/decoded image consumption, loose-file delivery and bounded startup
failures. It covers two independently replaceable sprite paths within R2.57–62; broader asset requirements
remain pending. A dedicated [generated image fixture](generated-assets.md)
exercises build-time/lazy generation and disk caching within R2.58–59.

## R1: product and initial scope

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R1.01 | Firm | Make trying new game ideas fast; this requirement does not prescribe a demonstration game. |
| R1.02 | Refined | Titan combines engine, framework/library, runtime and agent-facing construction roles. Rust authoring comes first; R1.16 and R1.28–34 leave additional declarative forms for later. |
| R1.03 | Firm | Start with human programmers using agents; eventually include agents directed by humans and non-programmers prompting agents. |
| R1.04 | Preference | Agent-free development should be possible, but it is not the initial priority. |
| R1.05 | Firm | Optimize for making good games, small or large, rather than limiting Titan to prototypes. |
| R1.06 | Firm | Develop Titan as an open-source project. It began as a private experiment; publication and release decisions remain maintainer responsibilities. |
| R1.07 | Open | The balance between a distinct opinionated identity and general-purpose engine remains unspecified. |
| R1.08 | Refined | Both 2D and 3D are intended; R2.03 selects 2D first rather than requiring simultaneous implementation. |
| R1.09 | Firm | Aim to accommodate most genres instead of making the initial demonstration genre a permanent limit. |
| R1.10 | Firm | Platform order is desktop, then mobile, then consoles. |
| R1.11 | Firm | Browser support matters early because agents often interact with browsers more effectively than desktop applications. |
| R1.12 | Firm | An agent should build from a developer's prompt and test headlessly while a human can play the same game code. Separate executions are acceptable; simultaneous interaction is not the first prerequisite. |
| R1.13 | Firm | Defer things not needed yet; build capabilities when they are needed. This sequences the vision rather than deleting future capabilities. |
| R1.14 | Preference | Bevy is the principal positive engine-design reference. |

## R1: agent workflow and authoring

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R1.15 | Firm | Accept natural requests ranging from precise to vague; agents should generally understand the human's intent rather than require a rigid prompt grammar. |
| R1.16 | Firm | Agents initially write Rust. Other authoring formats may come later; Rust's broad model familiarity is a reason for starting there. |
| R1.17 | Firm | Agents directly modify the game's actual source of truth. |
| R1.18 | Refined | Universal stable textual identifiers remain undecided. R2.26–27 affirm optional names/paths for entities, not identifiers for every object and property. |
| R1.19 | Refined | R2.34–40 define inspection metadata and protocol direction; a generated whole-game manifest remains open in R2.18. |
| R1.20 | Firm | Provide a well-designed Titan CLI that can communicate with a server inside the actual game process; command names are not prescribed by this requirement. |
| R1.21 | Firm | A CLI plus an agent skill is sufficient. An additional MCP service is not required. |
| R1.22 | Firm | Runtime failures should include agent-oriented diagnostics and suggested repairs. |
| R1.23 | Firm | Agents should query running-game information in structured form, including entities, component values, collisions, performance and logs. |
| R1.24 | Firm | Agents should control the running game and capture screenshots automatically. |
| R1.25 | Firm | Visual verification is a core workflow. |
| R1.26 | Firm | Repository-local discoverability is critical because some agents lack web access. |
| R1.27 | Open | Context-window efficiency is important; the mechanism, including capability maps or compact generated documentation, was not selected. |
| R1.28 | Firm | A game project is a compiled Rust program, not necessarily content loaded into a prebuilt Titan executable. |
| R1.29 | Firm | The engine core is Rust. |
| R1.30 | Firm | Gameplay starts in Rust, with possible scene formats later. |
| R1.31 | Firm | Pursue iteration speed, native performance and compile-time safety together. |
| R1.32 | Preference | Hot reload would be useful but is not required initially. Well-structured code and manageable compile times should support iteration meanwhile. |
| R1.33 | Preference | Scenes start in code; file-based scenes are a later direction. |
| R1.34 | Open | Authority depends on the authoring form a project uses; no universal code-versus-scene precedence was chosen. Avoid competing sources of truth. |
| R1.35 | Open | Metadata/generated metadata files are acceptable. The broader policy for generated files, especially gameplay source, was not settled. |
| R1.36 | Open | Everything should be expressible and documented; the balance between API constraints and flexibility remains open. |
| R1.37 | Open | The balance between simple explicit operations and compact abstractions remains undecided. |
| R1.38 | Refined | Support game-defined custom components and ECS authoring. A data-driven component-definition language is not part of this requirement. |

## R1: architecture and content

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R1.39 | Firm | Use an ECS architecture. |
| R1.40 | Firm | Build a custom ECS. |
| R1.41 | Refined | Composable libraries and an easy-to-use high-level API were initially a preference; R2.42 affirms the layered approach. |
| R1.42 | Firm | Prefer building systems ourselves unless an external dependency is sufficiently universal that reimplementation is unreasonable; `serde` is the example. R2.61 also permits excellent format libraries. |
| R1.43 | Firm | Start with `wgpu`; direct native Metal and Vulkan backends are intended later, not selected initial work. |
| R1.44 | Firm | Make low-level rendering control possible while preferring high-level APIs for ordinary use. |
| R1.45 | Preference | Deterministic simulation is the intended foundation; R1.58 and R2.52 reinforce deterministic testing and recording. |
| R1.46 | Firm | Headless execution is required from the beginning. |
| R1.47 | Firm | Design save/load and serialization early despite having no backward-compatibility requirement. This does not decide that every reflected type must serialize. |
| R1.48 | Preference | Subsystems should be replaceable at library boundaries; the higher-level framework may be more opinionated. Exact boundaries remain open. |
| R1.49 | Open | The initial playable slice did not prescribe a complete subsystem list. Audio, physics, animation and broader UI capabilities require separately selected scope. |
| R1.50 | Firm | Support asset-free prototyping through code-generated placeholder primitives, textures and simple sounds. |
| R1.51 | Open | Reusable gameplay primitives such as health, movement, cameras, triggers, timers and state machines may belong in the high-level framework; their inclusion and scope remain undecided. |
| R1.52 | Firm | UI should use the same entity/component model as the game world. The first text/button implementation is documented in the UI guide; broader UI capabilities remain future work. |
| R1.53 | Firm | Multiplayer in competitive, cooperative and local forms is intended. This is a long-term ambition informed by faster agent-assisted development, not a schedule to build every form immediately. |
| R1.54 | Firm | Procedural generation is important. |
| R1.55 | Firm | Titan should not prescribe where assets originate; provide procedural-generation APIs and CSG with Boolean operations. |

## R1: verification and project policy

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R1.56 | Refined | Proof of correctness was initially unspecified; R2.49 chooses evidence appropriate to the feature. |
| R1.57 | Preference | Projects should generally be runnable and testable entirely from the command line, subject to project needs; this was not an unconditional rule for every possible project. |
| R1.58 | Firm | Simulation tests should advance exact fixed frame counts deterministically. |
| R1.59 | Preference | Tests should assert entity state, pixels, collisions and emitted events where appropriate, not necessarily all of them for every feature. |
| R1.60 | Firm | Automatically produce compact failure diagnostics, including logs, screenshots, world state and timings, with a way to disable them. R2.54 additionally requests an always-output option. |
| R1.61 | Firm | Make edit/build/run/feedback as fast as practical; no numerical latency threshold was selected. |
| R1.62 | Refined | An editor was never excluded; it is not the priority. An in-game debug inspector may be developed if there is sufficient interest. |
| R1.63 | Firm | Support inspection and mutation of a running game. R2.39 refines the initial transport toward localhost. |
| R1.64 | Open | The acceptable technical-debt tradeoff between speed and architectural quality was not decided. |
| R1.65 | Firm | Games can pin engine versions. Current repository examples and tests should remain working when migrations change the current engine. |
| R1.66 | Firm | Freely delete or redesign APIs when better approaches emerge; provide migration guides for especially impactful changes. |
| R1.67 | Firm | Start with generic pieces, then build concrete games and add engine capabilities those games require; also address feature requests and bug reports. |
| R1.68 | Instruction | Explain why the current architecture works as it does, but do not maintain a separate chronological decision diary. Git already records history. |
| R1.69 | Firm | Automated formatting, linting, tests and architectural checks matter from the beginning; CI is very important. Specific architectural checks were not selected. |
| R1.70 | Firm | Wait for measurement before performance optimization, except for early choices that would be costly to reverse. |
| R1.71 | Firm | Rethink the approach if iterating on a game with an AI agent does not feel natural. |

## R2: demonstration and execution modes

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R2.01 | Preference | The suggested first demonstration is a small procedural 2D RPG; the original choice was tentative, not a permanent genre restriction. |
| R2.02 | Firm | Two initial workflow examples are starting a procedural 2D RPG with generated pixel art and improving that art. These examples guide verification without defining additional requirements. |
| R2.03 | Firm | Implement 2D first. |
| R2.04 | Open | The minimum graphical feature set was not specified. |
| R2.05 | Preference | The first milestone should probably use primitive/code-generated assets exclusively; this is not a permanent restriction on asset sources. |
| R2.06 | Firm | Support both games compiled to WASM and a browser-based inspector for native games. |
| R2.07 | Preference | Both browser approaches probably belong early, with a slight preference toward actual WASM compilation. |
| R2.08 | Firm | Headless use includes simulation without graphics and off-screen rendering for captures. |
| R2.09 | Firm | Headless tests must work on CI machines without a GPU. |
| R2.10 | Firm | Playable and headless modes may be separate runs of identical game code. |
| R2.11 | Preference | A human playing while an agent observes and modifies the same live process is strongly desired. Do not confuse this with a requirement that every early test share a live process. |

## R2: agent workflow

These requirements describe the intended authoring and runtime inspection loop.

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R2.12 | Firm | The proposed edit/check/play-or-test/start-inspection/input-or-step/query-and-capture/evaluate loop is the intended workflow. Example CLI command spellings are not fixed requirements. |
| R2.13 | Preference | Cargo stays supported; the Titan CLI may be preferred. |
| R2.14 | Firm | Ordinary Cargo use, including running the game, must remain supported. |
| R2.15 | Firm | The CLI supports both human-readable and machine-readable output. |
| R2.16 | Firm | Every CLI operation has stable structured output. |
| R2.17 | Firm | The agent skill should mainly teach the Titan CLI. Engine docs and game code/docs should make the remaining model understandable. |
| R2.18 | Open | Whether a compact generated game manifest makes sense remains undecided. |
| R2.19 | Firm | The CLI should locate and attach to an already-running game. |
| R2.20 | Preference | Runtime mutation is desired for debugging; a separate explicitly enabled mode was suggested tentatively. Current mutation-policy behavior is documented separately in the inspection docs. |
| R2.21 | Firm | Support game-defined commands, not only raw field edits. |

## R2: ECS and inspection

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R2.22 | Firm | Components should be ordinary Rust structs with a component derive. |
| R2.23 | Preference | Begin with Bevy-like typed system parameters; remain open to exploring other authoring styles later. |
| R2.24 | Preference | Prefer automatic reflection/inspection support through a derive macro over manually registering every component. |
| R2.25 | Open | Whether all inspectable components must also be serializable remains undecided. |
| R2.26 | Firm | Human-readable entity names or paths are optional. |
| R2.27 | Firm | Keep unnamed entities cheap while allowing important entities to have useful persistent names/paths. |
| R2.28 | Firm | Eventually run systems in parallel automatically when their data access permits. This is a design requirement, not a claim about the current executor. |
| R2.29 | Firm | Make the choice between determinism and maximum parallelism configurable. |
| R2.30 | Firm | Familiar explicit default stages belong only in the higher-level framework and must be customizable. Do not impose those stages on low-level ECS consumers. |
| R2.31 | Firm | Support both modest worlds and very large games, including millions of lightweight entities. This is an ambition, not an established capacity benchmark. |
| R2.32 | Preference | Prefer buffered structural changes; explicit synchronization points should be possible with their performance cost understood. |
| R2.33 | Preference | Eventual rollback snapshots are strongly desired for multiplayer and deterministic debugging. |
| R2.34 | Firm | Reflection metadata is acceptable for tool-exposed types; internal types may remain opaque. |
| R2.35 | Firm | Allow descriptions, ranges, units and editor hints without requiring every enrichment. |
| R2.36 | Firm | Keep documentation on Rust types and fields so it can feed Rust docs, agent context and a future editor. |
| R2.37 | Open | The broader requirement for explaining rejected writes was left unresolved. The [inspection documentation](inspection.md) describes implemented structured validation errors; that implementation does not settle the broader design requirement. |
| R2.38 | Preference | A structured in-code request model should underlie other query forms. JSON, CLI flags and possibly a query language were considered; a separate query language was not firmly selected. |
| R2.39 | Firm | A localhost JSON protocol over HTTP/WebSocket is an acceptable initial direction; both transports are not required. |
| R2.40 | Firm | The inspection protocol should be public and documented for third-party tools. |

## R2: API structure and verification

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R2.41 | Firm | A Bevy-like `App` builder with plugins and schedule/system registration is an acceptable minimal game API. The exact illustrative snippet is not an immutable API contract. |
| R2.42 | Firm | Provide the high-level `App` API while exposing lower-level crates independently. |
| R2.43 | Open | Choose crate-versus-module boundaries according to how certain responsibilities are; no blanket “every subsystem starts as a crate” rule was selected. |
| R2.44 | Firm | Games should be able to disable every major subsystem they do not use. |
| R2.45 | Firm | Support stable Rust; nightly use may also be possible. Nightly-only enhancements were not specifically requested and must not be assumed necessary. |
| R2.46 | Firm | Isolate unsafe code and explain its safety invariants very clearly. |
| R2.47 | Preference | Support multithreading soon, though not necessarily immediately. |
| R2.48 | Preference | An async task system is desired for assets and background work, but need not precede having assets to load. |
| R2.49 | Firm | Select semantic assertions, screenshots, input recordings or combinations according to the feature. |
| R2.50 | Preference | Start with ordinary Rust tests; Titan-specific hooks may make them easier. |
| R2.51 | Firm | Ergonomic test helpers for building a game, advancing frames, sending input, finding entities, asserting fields and capturing images are desirable. Exact example method names are illustrative. |
| R2.52 | Firm | Support deterministic input recordings that can also be replayed interactively. Headless recording verification alone is not full coverage of this requirement. |
| R2.53 | Firm | Support both exact-pixel comparisons and configurable perceptual tolerances. |
| R2.54 | Firm | Write diagnostic bundles on failure by default, with an option to write them always. R1.60 also requires an off switch. |
| R2.55 | Firm | Support both simulation-frame budgets and wall-clock timeouts for hanging tests. |
| R2.56 | Open | Whether tests should assert performance budgets remains undecided. |

## R2: assets and project policy

| ID | Standing | Requirement, preference, or unresolved choice |
| --- | --- | --- |
| R2.57 | Firm | Code-generated meshes, textures, materials, sounds and animations should be first-class assets using the same interface as file-backed assets. |
| R2.58 | Firm | Support procedural generation at compile/build time, startup and lazily at runtime. |
| R2.59 | Firm | Generated assets should be cacheable on disk. |
| R2.60 | Firm | CSG is intended both for runtime gameplay and asset construction. |
| R2.61 | Firm | Import common formats through external libraries only when they are very well maintained and high quality; otherwise implement the needed support ourselves. |
| R2.62 | Firm | An engine-native asset format is intended eventually; its design is not yet selected. |
| R2.63 | Firm | Titan is dual-licensed under MIT and Apache-2.0; dependencies must be compatible. |
| R2.64 | Firm | Early optimization may focus on the primary development platform while preserving a portable architecture. |
| R2.65 | Firm | The initial reference development platform is macOS on Apple Silicon (M5 Pro). This is a reference environment, not a minimum supported machine or numerical performance target. |
| R2.66 | Firm | Use GitHub Actions, with CI from the beginning. |
| R2.67 | Firm | Changes must pass formatting, Clippy with warnings denied, unit tests and headless integration tests. |
| R2.68 | Firm | Make releases frequent so games can pin versions; no precise cadence or automatic publication permission follows. |
| R2.69 | Firm | Treat examples as tested products that compile against the current engine. |
| R2.70 | Instruction | Document the agreed design before selecting substantial implementation work. Routine changes do not require repeating the initial design process. |
