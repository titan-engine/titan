/* Checks that vendored flecs works with our compiler flags, sanitizers and on every target. */
#include "test.h"

#include <flecs.h>
#include <string.h>

ECS_STRUCT(Position, {
    float x;
    float y;
});

ECS_STRUCT(Velocity, {
    float x;
    float y;
});

static void Move(ecs_iter_t *it) {
    Position *p = ecs_field(it, Position, 0);
    const Velocity *v = ecs_field(it, Velocity, 1);
    for (int i = 0; i < it->count; i++) {
        p[i].x += v[i].x * it->delta_time;
        p[i].y += v[i].y * it->delta_time;
    }
}

static void test_system_moves_entities(void) {
    ecs_world_t *world = ecs_init();
    ECS_META_COMPONENT(world, Position);
    ECS_META_COMPONENT(world, Velocity);
    ECS_SYSTEM(world, Move, EcsOnUpdate, Position, [in] Velocity);

    ecs_entity_t e = ecs_new(world);
    ecs_set(world, e, Position, {0, 0});
    ecs_set(world, e, Velocity, {1, 2});
    for (int frame = 0; frame < 4; frame++) {
        ecs_progress(world, 0.25f);
    }
    const Position *p = ecs_get(world, e, Position);
    CHECK(p != nullptr && p->x == 1.0f && p->y == 2.0f);
    ecs_fini(world);
}

static void test_script_and_json(void) {
    ecs_world_t *world = ecs_init();
    ECS_META_COMPONENT(world, Position);
    CHECK(ecs_script_run(world, "smoke", "hero { Position: {x: 10, y: 20} }", nullptr) == 0);

    ecs_entity_t hero = ecs_lookup(world, "hero");
    CHECK(hero != 0);
    char *json = ecs_entity_to_json(world, hero, nullptr);
    CHECK(json != nullptr && strstr(json, "\"Position\":{\"x\":10, \"y\":20}") != nullptr);
    ecs_os_free(json);
    ecs_fini(world);
}

int main(void) {
    RUN(test_system_moves_entities);
    RUN(test_script_and_json);
    return TEST_RESULT();
}
