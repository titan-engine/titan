# Open questions for milestone 2

These questions guide the starter and independent arena-game exercise. Resolve
them with the smallest working consumer, then move the resulting behavior into
its usage documentation and remove the answered question. The execution order
is in the [implementation plan](implementation-plan.md).

## Reusable setup

- What is the smallest public boundary between game construction, logical input,
  render extraction, and native/browser host setup?
- Which current runner or build-script assumptions are tied to the RPG, and which
  can be shared without introducing an oversized framework?
- How should a copied starter declare its local or pinned Titan dependency and
  build its browser entry point without assuming the engine workspace layout?

## Game authoring

- Do arena movement, pursuit, spawning, and collision systems fit the existing
  typed parameters and queries? If not, what exact missing operation blocks them?
- Which collision helpers, if any, merit reuse after the first game-local version?
- What health, timing, and outcome presentation can use existing rendering
  facilities, and what minimal addition is justified if they are insufficient?

## Inspection and diagnosis

- Which arena values and commands are needed to diagnose failures, and where does
  manual field/metadata registration become a demonstrated authoring obstacle?
- Can the independent agent discover all build, run, replay, and diagnostic steps
  from local guidance without explanations specific to the RPG?
- Are the existing request history and execution budgets sufficient for the arena
  scenarios, including failures? Which missing evidence actually prevents a fix?

## Validation and iteration cost

- What deterministic scenarios cover both victory and defeat without making the
  tests slow or brittle?
- What are the measured starter build and game iteration times, and where is the
  largest avoidable delay? Add performance policy only after measuring it.
- After the independent exercise, which demonstrated limitation deserves the next
  milestone rather than another isolated workaround?
