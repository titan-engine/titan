# 0011. Agents and humans get the same capabilities

- Status: Accepted
- Date: 2026-10-13

## Context

Titan should be as easy for AI agents to use as for humans. Agents work through text, commands and images; they
cannot click through a GUI editor, and they need fast, reliable feedback.

## Decision

- Text-first data: scenes, prefabs and asset metadata are diffable text with documented schemas.
- Headless and deterministic: any game can run without a window for N frames at a fixed timestep, with results
  that can be asserted in tests. Screenshots can be captured to PNG headlessly.
- Live inspection protocol: a running game can be queried and modified (entities, components, systems), stepped
  frame by frame, and its logs read, via CLI, HTTP and MCP. Builds on flecs reflection and its REST API.
- The editor (later) is a client of that same protocol. There is no editor-only capability.
- Input recording and replay for reproducing bugs.
- Errors and logs are structured and say what to do next.

## Consequences

- Every feature needs a headless path and must be reachable through the protocol, not just the UI.
- Tests and agents use the same entry points, so agent workflows are continuously tested.
