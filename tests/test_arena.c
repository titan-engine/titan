#include "test.h"
#include "titan/core.h"

static void test_alloc_alignment_and_zeroing(void) {
    titan_arena *arena = titan_arena_create(256);
    REQUIRE(arena != nullptr);
    for (size_t align = 1; align <= 256; align *= 2) {
        unsigned char *p = titan_arena_alloc(arena, 3, align);
        REQUIRE(p != nullptr);
        CHECK(((uintptr_t)p & (align - 1)) == 0);
        CHECK(p[0] == 0 && p[1] == 0 && p[2] == 0);
    }
    titan_arena_destroy(arena);
}

static void test_large_allocation_gets_own_block(void) {
    titan_arena *arena = titan_arena_create(64);
    REQUIRE(arena != nullptr);
    unsigned char *big = titan_arena_alloc(arena, 10000, 16);
    REQUIRE(big != nullptr);
    big[9999] = 1; /* AddressSanitizer catches it if the block is too small */
    titan_arena_destroy(arena);
}

static void test_invalid_alignment_fails(void) {
    titan_arena *arena = titan_arena_create(0);
    REQUIRE(arena != nullptr);
    CHECK(titan_arena_alloc(arena, 8, 0) == nullptr);
    CHECK(titan_arena_alloc(arena, 8, 3) == nullptr);
    titan_arena_destroy(arena);
}

static void test_array_overflow_fails(void) {
    titan_arena *arena = titan_arena_create(0);
    REQUIRE(arena != nullptr);
    CHECK(titan_arena_alloc_array(arena, SIZE_MAX / 2, 4, 4) == nullptr);
    CHECK(titan_arena_alloc(arena, SIZE_MAX, 8) == nullptr);
    titan_arena_destroy(arena);
}

static void test_typed_new_and_reset(void) {
    typedef struct {
        float x, y, z;
    } vec3;
    titan_arena *arena = titan_arena_create(128);
    REQUIRE(arena != nullptr);
    for (int round = 0; round < 3; round++) {
        vec3 *v = titan_arena_new(arena, vec3, 100);
        REQUIRE(v != nullptr);
        CHECK(v[99].z == 0.0f);
        v[99].z = 1.0f;
        titan_arena_reset(arena);
    }
    titan_arena_destroy(arena);
}

int main(void) {
    RUN(test_alloc_alignment_and_zeroing);
    RUN(test_large_allocation_gets_own_block);
    RUN(test_invalid_alignment_fails);
    RUN(test_array_overflow_fails);
    RUN(test_typed_new_and_reset);
    titan_arena_destroy(nullptr);
    return TEST_RESULT();
}
