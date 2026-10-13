# 0009. Scripting-first games, native code as a peer

- Status: Accepted (scripting language still open)
- Date: 2026-10-13

## Context

If games are written in the engine's language, everyone pays engine compile times and must know C. Agents and
humans iterate fastest with a scripting language and hot reload. But native code must not be an afterthought.

## Decision

- Games are primarily data (scenes, assets) plus scripts, running on a prebuilt engine with hot reload.
- Native code is a first-class peer: the engine exposes one API (components, systems, queries, commands) through
  the C API, script bindings and the remote protocol (ADR 0011). Native plugins use the same API as scripts.
- Components defined by scripts are real flecs components with reflection, indistinguishable from native ones.

Open question: the scripting language. Candidates, both pure C and web-compatible without emscripten:
Lua 5.4 (tiny, fast, agents know it well) and QuickJS-ng (JavaScript, with TypeScript compiled to JS).
To be decided in a follow-up ADR.

## Consequences

- Making a game requires no C compiler.
- The engine API must be described in a form that generates bindings and docs, so all front ends stay in sync.
