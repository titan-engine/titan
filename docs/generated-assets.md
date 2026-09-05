# Generated image cache fixture

Issue [#9](https://github.com/titan-engine/titan/issues/9) exercises one
deterministic generated PNG in `fixtures/generated-asset`. The fixture owns the
generator, its inputs/version, cache files and lazy lifetime. Titan continues to
consume the result through its existing RGBA `Image` boundary. This is a native
build/runtime tooling exercise; no engine asset registry, RPG integration, hot
reload, renderer change or additional format is introduced.

## Verification

The fixture is a workspace member, so the ordinary workspace formatting, tests
and Clippy gates cover it. `python3 scripts/test-generated-assets.py` also drives
separate processes to verify persistent cache reuse and recovery. The required
native CI job runs this process-level exercise alongside existing game coverage.
