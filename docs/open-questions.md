# Open questions

Milestone 2's authoring, inspection, diagnosis and standalone setup questions are
answered by the accepted [arena exercise](arena-exercise.md),
[starter verification](starter-verification.md) and
[fresh arena verification](arena-verification.md). Movement, pursuit, collision,
health and outcome presentation needed no engine changes. Existing request
history and diagnostics were sufficient to diagnose the failed route. No shared
collision helper or reflection expansion was justified.

Host setup consolidation is complete, including remote CI. The next authorized
objective is an arena dash ability with measured edit/build/run/inspect latency.
The [implementation plan](implementation-plan.md) tracks execution; the
[host setup audit](host-setup-audit.md) records completed consolidation evidence.
Keep broader framework, camera and platform features demand-driven.

Remaining questions require evidence from future game iteration:

- Does full native lifecycle ownership become a repeated customization burden,
  beyond the small surface and input responsibilities now identified?
- Which part of edit/build/run latency dominates ordinary game changes after
  shared setup is removed? Warm measurements do not establish clean-build gains.
- Does a future game need live inspection of the player instance or browser
  diagnostic bundle export enough to justify changing the current host model?
