/* The allocator module: one of the few places allowed to call malloc/free (see AGENTS.md). */
#include "titan/core.h"

#include <stdlib.h>
#include <string.h>

typedef struct titan_arena_block {
    struct titan_arena_block *prev;
    size_t size; /* usable bytes in data */
    size_t used;
    alignas(max_align_t) unsigned char data[];
} titan_arena_block;

struct titan_arena {
    titan_arena_block *current;
    size_t block_size;
};

titan_arena *titan_arena_create(size_t block_size) {
    titan_arena *arena = malloc(sizeof *arena);
    if (arena == nullptr) {
        return nullptr;
    }
    arena->current = nullptr;
    arena->block_size = block_size != 0 ? block_size : TITAN_ARENA_DEFAULT_BLOCK_SIZE;
    return arena;
}

void titan_arena_destroy(titan_arena *arena) {
    if (arena == nullptr) {
        return;
    }
    titan_arena_block *block = arena->current;
    while (block != nullptr) {
        titan_arena_block *prev = block->prev;
        free(block);
        block = prev;
    }
    free(arena);
}

/* Bump-allocates from `block`, or returns nullptr if it does not fit. */
static void *block_alloc(titan_arena_block *block, size_t size, size_t align) {
    uintptr_t at = (uintptr_t)(block->data + block->used);
    size_t padding = (size_t)(-at & (uintptr_t)(align - 1));
    size_t available = block->size - block->used;
    if (padding > available || size > available - padding) {
        return nullptr;
    }
    unsigned char *result = block->data + block->used + padding;
    block->used += padding + size;
    memset(result, 0, size);
    return result;
}

void *titan_arena_alloc(titan_arena *arena, size_t size, size_t align) {
    if (align == 0 || (align & (align - 1)) != 0) {
        return nullptr;
    }
    if (arena->current != nullptr) {
        void *result = block_alloc(arena->current, size, align);
        if (result != nullptr) {
            return result;
        }
    }

    /* New block, large enough for this allocation plus worst-case alignment padding. */
    if (size > SIZE_MAX - align) {
        return nullptr;
    }
    size_t needed = size + align;
    size_t block_size = needed > arena->block_size ? needed : arena->block_size;
    if (block_size > SIZE_MAX - sizeof(titan_arena_block)) {
        return nullptr;
    }
    titan_arena_block *block = malloc(sizeof(titan_arena_block) + block_size);
    if (block == nullptr) {
        return nullptr;
    }
    block->prev = arena->current;
    block->size = block_size;
    block->used = 0;
    arena->current = block;
    return block_alloc(block, size, align);
}

void *titan_arena_alloc_array(titan_arena *arena, size_t count, size_t size, size_t align) {
    if (size != 0 && count > SIZE_MAX / size) {
        return nullptr;
    }
    return titan_arena_alloc(arena, count * size, align);
}

void titan_arena_reset(titan_arena *arena) {
    titan_arena_block *keep = arena->current;
    if (keep == nullptr) {
        return;
    }
    titan_arena_block *block = keep->prev;
    while (block != nullptr) {
        titan_arena_block *prev = block->prev;
        free(block);
        block = prev;
    }
    keep->prev = nullptr;
    keep->used = 0;
}
