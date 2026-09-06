# Open questions

These are durable unresolved design choices. Their selection and follow-up work
are tracked as Proposed issues in [Titan Development](https://github.com/orgs/titan-engine/projects/1).
Use the board for priority, ownership and dependencies; update this reference when
a decision is made. Recording a question does not approve implementation.

Milestone 2's authoring, inspection, diagnosis and standalone setup questions are
answered by the accepted [arena exercise](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/arena-exercise.md),
[starter verification](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/starter-verification.md) and
[fresh arena verification](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/arena-verification.md). Movement, pursuit, collision,
health and outcome presentation needed no engine changes. Existing request
history and diagnostics were sufficient to diagnose the failed route. No shared
collision helper or reflection expansion was justified.

Host setup consolidation is complete, including remote CI. The arena dash,
input cancellation consolidation and live-player inspection are implemented.
Difficulty settings remain a future possibility. See the
[live-player evidence](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/live-player.md).
The [verification guide](verification.md) defines quality gates; the
[host setup audit](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/host-setup-audit.md) records completed consolidation evidence.
The [first entity-based UI slice](ui.md) now covers both games and pointer-driven
restart. The [quest journal](journal.md) now exercises column layout, bounded bitmap text
and scoped focus; broader layout/typography remain future work;
the [save/load boundary](save-load.md) separates those derived entities from
persistent gameplay state.
Keep broader framework, camera and platform features demand-driven.

Remaining questions require evidence from future game iteration:

- Does full native lifecycle ownership become a repeated customization burden,
  beyond the small surface and input responsibilities now identified?
- Will larger games make build latency a practical constraint? The [dash
  measurements](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/arena-dash.md) put browser packaging/rebuild first among measured
  stages (1.227s with cached dependencies), with inspection around 6ms. This small
  workload does not establish clean-build costs or justify engine optimization.
- Will a second game need different live-host customization? Arena now proves
  same-instance inspection and consumed-input export/replay. A generic live app
  framework or automatic browser diagnostic bundles remain unselected.

## Design choices still open

The [design requirements](design-requirements.md) distinguish firm commitments
from tentative design directions. Completed game milestones do not resolve all of those broader questions. These are unresolved
choices, not a queue of authorized implementation tasks:

- **Product identity:** how opinionated should Titan be as a whole, beyond the
  agreed convenient high-level framework over composable libraries? (R1.7)
- **Textual identifiers:** should every game object and property have a stable
  textual identifier? That universal requirement was left undecided. The later
  acceptance of optional entity names/paths does not require identifiers for all
  objects or properties. (R1.18; R2.26–27)
- **Authoring and generated files:** metadata generation is acceptable, but the
  policy for other generated project files remains undecided. Code/file authority
  should be clear per project; no universal scene-file design has been chosen.
  (R1.34–35)
- **API style and replacement:** how much constraint versus flexibility, explicit
  operations versus abstractions, and subsystem replacement should each layer
  provide? Expressibility and documentation are priorities; the replacement
  direction is tentative beyond required subsystem disableability. (R1.36–37,
  R1.48; R2.44)
- **Reusable gameplay primitives:** should health, movement, cameras, triggers,
  timers and state machines be framework features? The high-level framework is a
  tentative home for them. This is separate from the firm requirement that UI
  share the entity/component model. (R1.51–52)
- **Generated capability summaries:** does a compact game manifest or capability
  map help discovery and context efficiency enough to justify it? Context-aware
  local documentation is required; that particular format is undecided.
  (R1.27; R2.18)
- **Reflection and serialization:** must every inspectable component also be
  serializable? The answer remains open; early save/load and serialization design
  does not settle that coupling. (R1.47; R2.25)
- **Query language:** should a dedicated query language join JSON requests and
  CLI flags? That possibility was tentative. JSON and CLI currently share the
  implemented typed request model. (R2.38)
- **Performance assertions:** should tests enforce performance budgets, and if so
  how should those budgets account for environment differences? Measurement and
  bounded test execution are agreed; performance assertions remain undecided.
  (R2.56)
- **Technical debt:** what tradeoffs are acceptable when iteration speed and
  architectural quality conflict? No general policy has been chosen.
  (R1.64)
