# flecs

- Source: https://github.com/SanderMertens/flecs
- Version: v4.1.6 (commit fb55f3c25660425cfe1bc4cf5e6bff8b3f18a9b8)
- Files: `distr/flecs.c`, `distr/flecs.h`, `LICENSE` (MIT)
- Local patches: none

## Updating

1. Copy `distr/flecs.c` and `distr/flecs.h` from the new release tag.
2. Update the version and commit above.
3. Run `just check`. On web, flecs is configured in `third_party/CMakeLists.txt` (addon list, `ECS_TARGET_POSIX`).
