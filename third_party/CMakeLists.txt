# Vendored third-party code: warnings off, but built with the same sanitizers as everything else.
# Adding anything here requires maintainer approval (docs/adr/0010-agent-workflow.md).

add_library(titan_flecs STATIC flecs/flecs.c)
target_include_directories(titan_flecs SYSTEM PUBLIC flecs)
# Line tables are enough for sanitizer stack traces into third-party code.
target_compile_options(titan_flecs PRIVATE -w $<$<CONFIG:Debug>:-gline-tables-only>)
# UBSan in trap mode for third-party code: same checks, but no per-check handler data, which
# roughly triples object size. Traps are still reported with the check and line, because
# ASAN_OPTIONS contains handle_sigtrap=1 (CMakePresets.json, justfile).
if(TITAN_SANITIZE MATCHES "undefined")
    target_compile_options(titan_flecs PRIVATE -fsanitize-trap=undefined)
endif()
# gnu99 like upstream: flecs's POSIX code (clock_gettime, pthreads) needs glibc's default feature macros.
set_target_properties(titan_flecs PROPERTIES C_STANDARD 99 C_EXTENSIONS ON)

if(CMAKE_SYSTEM_NAME STREQUAL "WASI")
    # No emscripten: flecs does not recognize plain wasm yet, so describe it as POSIX-like,
    # drop flecs's own OS layer (threads, dlopen, sockets) and pick addons explicitly.
    target_compile_definitions(titan_flecs PUBLIC
        ECS_TARGET_POSIX FLECS_CUSTOM_BUILD FLECS_NO_OS_API_IMPL
        FLECS_SYSTEM FLECS_PIPELINE FLECS_TIMER FLECS_META FLECS_JSON FLECS_SCRIPT FLECS_PARSER
        FLECS_QUERY_DSL FLECS_MODULE FLECS_DOC FLECS_LOG FLECS_STATS FLECS_UNITS FLECS_ALERTS FLECS_METRICS)
    target_include_directories(titan_flecs PRIVATE "${PROJECT_SOURCE_DIR}/src/platform/web/compat")
endif()
