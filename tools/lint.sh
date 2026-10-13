#!/usr/bin/env bash
# Project rules that compilers and clang-tidy do not enforce. See AGENTS.md ("Memory" and "Strings").
set -euo pipefail
cd "$(dirname "$0")/.."

status=0
fail() { echo "lint: $1"; status=1; }

# Unbounded or error-prone libc string functions.
banned='\b(strcpy|strcat|sprintf|vsprintf|gets|strtok|strncpy|strncat|atoi|atol|atof)\s*\('
if grep -rnE "$banned" include src tests fuzz; then
    fail "banned function (use titan_str / snprintf / strtol-style parsing instead)"
fi

# Raw heap allocation is only allowed in allocator modules and platform code.
alloc='\b(malloc|calloc|realloc|free)\s*\('
allowed='^(src/core/arena\.c|src/platform/)'
if grep -rnE "$alloc" include src tests fuzz | grep -vE "$allowed"; then
    fail "raw malloc/free outside an allocator module (use an arena, or ask before adding an allocator)"
fi

exit $status
