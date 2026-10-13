/* Fuzzes the flecs script parser and interpreter, which will read text written by humans and
 * agents. A flecs assert triggered by script text (not API misuse) counts as a bug. */
#include <flecs.h>
#include <stdint.h>
#include <string.h>

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    static char script[1 << 16];
    if (size >= sizeof script) {
        return 0;
    }
    memcpy(script, data, size);
    script[size] = '\0';

    ecs_log_set_level(-4); /* parse errors are expected; keep output quiet */
    /* ecs_mini skips importing every addon; the script module imports meta (reflection) itself. */
    ecs_world_t *world = ecs_mini();
    ECS_IMPORT(world, FlecsScript);
    ecs_script_run(world, "fuzz", script, nullptr);
    ecs_fini(world);
    return 0;
}
