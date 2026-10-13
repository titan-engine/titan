#pragma once
/* Titan core: basic types and memory. Conventions are documented in AGENTS.md. */

#include <stdalign.h>
#include <stddef.h>
#include <stdint.h>

/* ---- Strings ------------------------------------------------------------------------------- */

/* A string slice: pointer + length, not necessarily NUL-terminated. Titan APIs take and
 * return titan_str instead of `const char *`. Convert at the boundary with C libraries. */
typedef struct titan_str {
    const char *ptr;
    size_t len;
} titan_str;

/* String literal to titan_str: TITAN_STR("hello"). Only accepts literals. */
#define TITAN_STR(lit) ((titan_str){.ptr = "" lit, .len = sizeof("" lit) - 1})

/* Wraps a NUL-terminated string (borrowed, no copy). nullptr yields the empty string. */
titan_str titan_str_from_cstr(const char *cstr);

bool titan_str_eq(titan_str a, titan_str b);

/* Sub-slice [start, end), clamped to the string's bounds. */
titan_str titan_str_slice(titan_str s, size_t start, size_t end);

/* ---- Arena allocator ------------------------------------------------------------------------ */

/* Arenas are the default way to allocate: allocate many objects, free them all at once.
 * Memory returned by an arena is zero-initialized and lives until reset or destroy. */
typedef struct titan_arena titan_arena;

#define TITAN_ARENA_DEFAULT_BLOCK_SIZE ((size_t)64 * 1024)

/* block_size 0 selects TITAN_ARENA_DEFAULT_BLOCK_SIZE. Returns nullptr on allocation failure. */
[[nodiscard]] titan_arena *titan_arena_create(size_t block_size);

/* Frees the arena and everything allocated from it. Accepts nullptr. */
void titan_arena_destroy(titan_arena *arena);

/* Allocates `size` bytes aligned to `align` (a power of two). Returns nullptr on failure. */
[[nodiscard]] void *titan_arena_alloc(titan_arena *arena, size_t size, size_t align);

/* Allocates `count * size` bytes, failing (nullptr) instead of overflowing. */
[[nodiscard]] void *titan_arena_alloc_array(titan_arena *arena, size_t count, size_t size, size_t align);

/* Typed helper: `vec3 *v = titan_arena_new(arena, vec3, 16);` */
#define titan_arena_new(arena, T, count) ((T *)titan_arena_alloc_array((arena), (count), sizeof(T), alignof(T)))

/* Invalidates all allocations but keeps the most recent block for reuse. */
void titan_arena_reset(titan_arena *arena);

/* Copies `s` into the arena, NUL-terminated for C interop (the terminator is not in len).
 * Returns {nullptr, 0} on allocation failure. */
[[nodiscard]] titan_str titan_str_copy(titan_arena *arena, titan_str s);

/* ---- Version -------------------------------------------------------------------------------- */

titan_str titan_version(void);
