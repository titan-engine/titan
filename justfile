# Titan task runner. `just` lists recipes; `just check` runs everything CI runs.

llvm := env("LLVM_ROOT", "/opt/homebrew/opt/llvm")
src_dirs := "include src tests fuzz"

# List recipes
default:
    @just --list

# Configure and build a preset (dev, tsan, release, fuzz, web)
build preset="dev":
    cmake --preset {{preset}} --log-level=WARNING
    cmake --build --preset {{preset}}

# Build and run the tests for a preset
test preset="dev": (build preset)
    ctest --preset {{preset}}

# Run a fuzz target, e.g. `just fuzz flecs_script 60`
fuzz target="flecs_script" seconds="30": (build "fuzz")
    mkdir -p build/fuzz/corpus/{{target}} build/fuzz/artifacts
    ASAN_OPTIONS=handle_sigtrap=1 ./build/fuzz/fuzz/fuzz_{{target}} -max_total_time={{seconds}} -artifact_prefix=build/fuzz/artifacts/ \
        build/fuzz/corpus/{{target}} fuzz/corpus/{{target}}

# Format all Titan sources in place
fmt:
    find {{src_dirs}} -name '*.[ch]' | xargs {{llvm}}/bin/clang-format -i

# Fail if any source is not formatted
fmt-check:
    find {{src_dirs}} -name '*.[ch]' | xargs {{llvm}}/bin/clang-format --dry-run --Werror

# Static analysis with clang-tidy (uses the dev build's compile_commands.json)
tidy: (build "dev")
    {{llvm}}/bin/run-clang-tidy -quiet -p build/dev -clang-tidy-binary {{llvm}}/bin/clang-tidy \
        "$(pwd)/(src|tests|fuzz)/"

# Project-specific rules (banned functions, malloc outside allocators)
lint:
    ./tools/lint.sh

# Everything CI runs. Run before opening a PR.
check: fmt-check lint (test "dev") (test "tsan") (test "web") tidy
    just fuzz flecs_script 20

# Delete all build output
clean:
    rm -rf build
