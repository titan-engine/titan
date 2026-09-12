# Development constraints

This document records the decisions guiding Titan’s development. It describes what we intend to build, not features that are already available.

## Rust and dependencies

Titan uses Rust edition 2024 and requires Rust 1.98.0 or newer. Keep the minimum supported Rust version close to the latest stable release and update the package metadata and build instructions together when it changes.

Cargo dependencies are not allowed. This includes regular, development, build, optional, and target-specific dependencies.

The Rust standard library is allowed. Prefer an equivalent API from `core` over `std` where practical, but do not complicate code to avoid the standard library. Keeping a future `no_std` mode possible is useful; implementing or guaranteeing one is not an early priority.

System libraries and frameworks, such as Metal and Vulkan, are allowed through handwritten bindings. Keep unsafe calls contained and document the ownership, lifetime, and threading requirements of the APIs we use.

GitHub Actions, debuggers, and other development tools are allowed. Platform SDKs and tools such as Vulkan validation layers can be useful, but optional tooling should stay optional. Document unavoidable compiler, linker, SDK, and system requirements for each supported build rather than claiming that all builds are SDK-free.

## Platforms and the first game

Bring macOS to a usable state first, then Linux, then Windows. Mobile platforms and consoles may follow later; neither is an early target.

The first game will be a small, old-school 2D platformer. Use that game to guide engine development rather than designing every subsystem in advance.

Game logic will be written in Rust. A scripting layer may be considered later, but is not part of the initial plan.

## Human and agent workflows

The visual editor and CLI must provide access to the same game-authoring operations and work with the same project data. Their interactions do not need to be identical: dragging an object in the editor and setting its position through a command are two ways to make the same change.

Agents should be able to inspect and change a project through the CLI without driving the editor’s UI. Humans should be able to make those changes naturally in the editor. Neither interface should be an afterthought.

Humans and agents initially take turns working on a project. Version control handles collaboration; the engine does not need simultaneous editing or its own collaboration service.

## Shaders

Authoring shaders should not require game authors to install a separate shader compiler. In the long term, the goal is to make shader authoring feel natural without requiring authors to manage platform differences themselves.

The shader authoring approach is not yet decided. Accepting backend-specific inputs, such as SPIR-V, GLSL, MSL, or WGSL, may help users reuse existing shaders. These are possibilities, not promised formats or a commitment to implement every graphics backend.

## Writing

Use clear, ordinary English throughout the project, including documentation, code comments, CLI messages, issues, and pull requests. Prefer concrete explanations over slogans, unnecessary jargon, and stock phrases. Write for developers and agents who have not seen the conversations that led to a decision.

Keep documentation and code in agreement. When a change affects documented behavior, commands, APIs, or requirements, update the documentation in the same pull request. Check that examples and instructions still work, and remove or correct outdated claims. Describe planned features as plans, not as existing capabilities. If code and documentation disagree, resolve the mismatch rather than assuming either is automatically correct.

Issues must contain or link to the information needed to understand and carry out the work. Do not rely on private conversations, machine-specific paths, local branch state, or unpublished notes. Use GitHub’s native sub-issue and blocking relationships rather than repeating them in issue bodies.

English is the initial project language. Localization may be considered if contributors want to help, but it is not an early priority.

## Decisions still to make

- How Rust game projects will build against Titan, including how that fits the Cargo dependency restriction if the engine is split into multiple crates.
- The shader authoring approach and which backend-specific inputs, if any, to support.
- The actual build and runtime requirements for each platform as support is implemented.

Resolve these questions in focused issues when the relevant work is planned. This document does not prescribe a full engine architecture.
