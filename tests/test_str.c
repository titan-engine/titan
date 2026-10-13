#include "test.h"
#include "titan/core.h"

static void test_eq(void) {
    CHECK(titan_str_eq(TITAN_STR("abc"), titan_str_from_cstr("abc")));
    CHECK(!titan_str_eq(TITAN_STR("abc"), TITAN_STR("abd")));
    CHECK(!titan_str_eq(TITAN_STR("abc"), TITAN_STR("ab")));
    CHECK(titan_str_eq(TITAN_STR(""), titan_str_from_cstr(nullptr)));
    CHECK(titan_str_eq((titan_str){0}, TITAN_STR("")));
}

static void test_slice_clamps(void) {
    titan_str s = TITAN_STR("hello world");
    CHECK(titan_str_eq(titan_str_slice(s, 6, 11), TITAN_STR("world")));
    CHECK(titan_str_eq(titan_str_slice(s, 6, 999), TITAN_STR("world")));
    CHECK(titan_str_slice(s, 20, 30).len == 0);
    CHECK(titan_str_slice(s, 5, 2).len == 0);
}

static void test_copy_is_nul_terminated(void) {
    titan_arena *arena = titan_arena_create(0);
    REQUIRE(arena != nullptr);
    titan_str copy = titan_str_copy(arena, titan_str_slice(TITAN_STR("hello world"), 0, 5));
    REQUIRE(copy.ptr != nullptr);
    CHECK(titan_str_eq(copy, TITAN_STR("hello")));
    CHECK(copy.ptr[5] == '\0');
    titan_arena_destroy(arena);
}

static void test_version(void) {
    CHECK(titan_version().len > 0);
}

int main(void) {
    RUN(test_eq);
    RUN(test_slice_clamps);
    RUN(test_copy_is_nul_terminated);
    RUN(test_version);
    return TEST_RESULT();
}
