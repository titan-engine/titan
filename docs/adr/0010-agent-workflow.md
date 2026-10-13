# 0010. Development workflow with agents

- Status: Accepted
- Date: 2026-10-13

## Context

Most code is written by AI agents running locally in parallel git worktrees. The maintainer reviews important
PRs and makes architecture decisions. The previous project ran agents autonomously and slowed down through compile
times, CI duration and disk usage.

## Decision

- Work happens in small PRs, one concern each, with a description of what, why, and how it was tested.
- `just check` must pass before a PR is opened.
- Maintainer review is required for changes to: `docs/adr/`, `third_party/`, `cmake/`, `CMakePresets.json`,
  `justfile`, `tools/`, CI configuration, public headers in `include/`, and any new dependency.
  Other PRs may merge once CI passes, at the maintainer's discretion.
- Enforcement: agents currently push with the maintainer's GitHub account, and GitHub does not allow approving
  your own PR, so CODEOWNERS cannot require the maintainer's review yet. Until agents get their own account or
  GitHub App, this rule holds by convention: agents never merge PRs that touch protected paths.
- New third-party code requires maintainer approval before work starts. Prefer writing our own over adding
  dependencies that bring build complexity or emscripten requirements.
- Each worktree builds in its own `build/`. Never point two worktrees at one build directory (stale binaries).
  Remove worktrees when their PR merges.
- CI budgets (to be enforced when CI exists): clean build time, test time, and binary size are reported per PR;
  regressions need justification.

## Consequences

- The maintainer's attention goes to decisions and core code; routine changes flow through CI.
- Agents must stop and ask (or propose an ADR) instead of silently changing protected areas.
