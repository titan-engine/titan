# Two-character puzzle adventure

This directory contains the selected first-slice [game design](design.md),
produced for [issue #80](https://github.com/titan-engine/titan/issues/80).
It is a design contract, not a runnable game or evidence of implemented physics.

The slice is two compact rooms played by switching between a high-jumping
character and a strong character who moves a heavy block. Both must reach each
exit. Rules, dimensioned layouts, solution routes and verification cases are in
the design; Rust game source will be the runtime authority when implemented.

Execution and future work live in [the adventure project](https://github.com/orgs/titan-engine/projects/2),
including the [skeleton](https://github.com/titan-engine/titan/issues/81),
[characters](https://github.com/titan-engine/titan/issues/82),
[first puzzle](https://github.com/titan-engine/titan/issues/83),
[combined puzzle](https://github.com/titan-engine/titan/issues/84),
[sequence](https://github.com/titan-engine/titan/issues/85),
[verification](https://github.com/titan-engine/titan/issues/86) and
[next-milestone planning](https://github.com/titan-engine/titan/issues/87).
These links describe issue ownership, not an additional implementation checklist.

The [standalone starter](../../starters/minimal/README.md) provides the public-API
host workflow; [collection room](../collection-room/README.md) is the existing
3D rendering, inspection and replay reference. This game owns its puzzle rules
and layouts without changing those examples or defining a general scene format.
