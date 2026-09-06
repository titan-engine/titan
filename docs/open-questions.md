# Open questions

These are durable unresolved design choices. Explore them in local conversations
or [GitHub Discussions](https://github.com/titan-engine/titan/discussions); create
an issue when there is concrete implementation or a bounded investigation to do.
Use [Titan Development](https://github.com/orgs/titan-engine/projects/1) for that
work's priority, ownership and dependencies. Update this reference when a decision
is made.

## Game iteration questions

These choices require evidence from game iteration:

- Does full native lifecycle ownership become a repeated customization burden,
  beyond the small surface and input responsibilities now identified?
- Will larger games make build latency a practical constraint? The [dash
  measurements](https://github.com/titan-engine/titan/blob/e4ff0dff2d02dfffa6bc085286798886a92e30e7/docs/arena-dash.md) put browser packaging/rebuild first among measured
  stages (1.227s with cached dependencies), with inspection around 6ms. This small
  workload does not establish clean-build costs or justify engine optimization.
- Will further games need different live-host customization? Arena now proves
  same-instance inspection and consumed-input export/replay. A generic live app
  framework or automatic browser diagnostic bundles remain unselected.

## Design choices still open

The [design requirements](design-requirements.md) distinguish firm commitments
from tentative design directions. Completed game milestones do not resolve all of those broader questions. These are unresolved
choices; concrete work belongs in issues:

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
