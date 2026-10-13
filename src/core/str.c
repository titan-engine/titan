#include "titan/core.h"

#include <string.h>

titan_str titan_str_from_cstr(const char *cstr) {
    if (cstr == nullptr) {
        return (titan_str){.ptr = "", .len = 0};
    }
    return (titan_str){.ptr = cstr, .len = strlen(cstr)};
}

bool titan_str_eq(titan_str a, titan_str b) {
    if (a.len != b.len) {
        return false;
    }
    /* memcmp with a null pointer is undefined even for length 0. */
    return a.len == 0 || memcmp(a.ptr, b.ptr, a.len) == 0;
}

titan_str titan_str_slice(titan_str s, size_t start, size_t end) {
    if (end > s.len) {
        end = s.len;
    }
    if (start > end) {
        start = end;
    }
    return (titan_str){.ptr = s.ptr + start, .len = end - start};
}

titan_str titan_str_copy(titan_arena *arena, titan_str s) {
    if (s.len == SIZE_MAX) {
        return (titan_str){.ptr = nullptr, .len = 0};
    }
    char *copy = titan_arena_alloc(arena, s.len + 1, 1);
    if (copy == nullptr) {
        return (titan_str){.ptr = nullptr, .len = 0};
    }
    if (s.len != 0) {
        memcpy(copy, s.ptr, s.len);
    }
    return (titan_str){.ptr = copy, .len = s.len};
}
