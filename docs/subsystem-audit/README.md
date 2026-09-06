# ECS subsystem boundary evidence

The historical [#38 audit](https://github.com/titan-engine/titan/issues/38)
distinguished explicit runtime construction from compile-time omission. Direct
`World` use created no app, renderer, inspector or host, while disabling default
features omitted PNG decoding rather than all non-ECS core modules. Native and
actual WASM external consumer assertions passed for both PNG feature variants.
This did not demonstrate `no_std`, browser rendering, Windows support or every
possible ECS workload; no binary-size, compile-time or allocation cost was measured.

The [original audit and reproduction instructions](https://github.com/titan-engine/titan/blob/1c885151a2e59b5f6212939c1658f4c49408273f/docs/subsystem-audit/README.md)
include the dependency table, commands, historical runner and consumer inputs.
Evidence revision `1c885151a2e59b5f6212939c1658f4c49408273f` measured engine
source `3271f2819c2a11a0e1fefa922f888e8864671800` on 2026-09-05, macOS arm64,
Rust 1.98.1 and Node 26.8.1. Reproduce in a disposable checkout following that
report; the harness is historical, not a maintained HEAD test.

These observations support the boundary discussions in
[#18](https://github.com/titan-engine/titan/issues/18),
[#52](https://github.com/titan-engine/titan/issues/52) and
[#70](https://github.com/titan-engine/titan/issues/70). They do not establish
current dependency closure or approve a crate split. See
[design requirements](../design-requirements.md) for the retained R2.44 boundary
and [ECS authoring](../ecs-authoring.md) for current public-API guidance.
