# Architecture Decision Records

Significant decisions are recorded here so humans and agents can follow them without re-deriving them.
See [0001](0001-record-architecture-decisions.md) for when to write one.

| ADR | Decision | Status |
|-----|----------|--------|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-c23-and-clang-only.md) | C23, LLVM clang only | Accepted |
| [0003](0003-flecs-as-ecs.md) | flecs as the ECS | Accepted |
| [0004](0004-build-system.md) | CMake + Ninja + just | Accepted |
| [0005](0005-bug-prevention.md) | Sanitizers, fuzzing, static analysis, conventions | Accepted |
| [0006](0006-graphics-webgpu-h.md) | Graphics through the standard `webgpu.h` API | Accepted |
| [0007](0007-web-without-emscripten.md) | Web builds without emscripten | Accepted |
| [0008](0008-own-platform-layer.md) | Our own platform layer | Accepted |
| [0009](0009-scripting-first.md) | Scripting-first games, native as a peer | Accepted (language open) |
| [0010](0010-agent-workflow.md) | Development workflow with agents | Accepted |
| [0011](0011-agent-human-parity.md) | Agents and humans get the same capabilities | Accepted |

## Template

```markdown
# NNNN. Title

- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD

## Context
What forces are at play, and what problem needs deciding.

## Decision
What we do.

## Consequences
What becomes easier, what becomes harder, what we must now watch for.
```
