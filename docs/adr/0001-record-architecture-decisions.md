# 0001. Record architecture decisions

- Status: Accepted
- Date: 2026-10-13

## Context

Titan is developed largely by AI agents, with a human maintainer reviewing important changes. Agents start each
task without memory of earlier discussions, so decisions that live only in chat history get re-litigated or
silently violated.

## Decision

Significant decisions are written down as short ADRs in `docs/adr/`. A decision is significant if it changes
the language, toolchain, build, dependencies, public API shape, platform support, or development process.

- Agents read the relevant ADRs before working in an area, and follow them.
- An agent that believes an ADR is wrong proposes a new ADR (status Proposed) in its own PR instead of working
  around it. Only the maintainer accepts ADRs.
- ADRs are not edited after acceptance except for status changes; a new ADR supersedes an old one.

## Consequences

Decisions survive across agent sessions. Changing direction costs one short document and a review.
