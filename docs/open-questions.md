# Open questions

Milestone 2's authoring, inspection, diagnosis and standalone setup questions are
answered by the accepted [arena exercise](arena-exercise.md),
[starter verification](starter-verification.md) and
[fresh arena verification](arena-verification.md). Movement, pursuit, collision,
health and outcome presentation needed no engine changes. Existing request
history and diagnostics were sufficient to diagnose the failed route. No shared
collision helper or reflection expansion was justified.

Host setup consolidation is complete, including remote CI. The arena dash
was accepted by the user. Difficulty settings remain a future possibility;
input cancellation consolidation and live-player inspection are now locally
verified. See the [live-player evidence](live-player.md).
The [implementation plan](implementation-plan.md) tracks execution; the
[host setup audit](host-setup-audit.md) records completed consolidation evidence.
Keep broader framework, camera and platform features demand-driven.

Remaining questions require evidence from future game iteration:

- Does full native lifecycle ownership become a repeated customization burden,
  beyond the small surface and input responsibilities now identified?
- Will larger games make build latency a practical constraint? The [dash
  measurements](arena-dash.md) put browser packaging/rebuild first among measured
  stages (1.227s with cached dependencies), with inspection around 6ms. This small
  workload does not establish clean-build costs or justify engine optimization.
- Will a second game need different live-host customization? Arena now proves
  same-instance inspection and consumed-input export/replay. A generic live app
  framework or automatic browser diagnostic bundles remain unselected.
