# Native toolchain: LLVM clang (not Apple clang, which lacks LeakSanitizer and libFuzzer).
# Search order: $LLVM_ROOT, Homebrew LLVM, then PATH. See docs/adr/0002-c23-and-clang-only.md.

set(_titan_llvm_hints "$ENV{LLVM_ROOT}/bin" /opt/homebrew/opt/llvm/bin /usr/local/opt/llvm/bin)

find_program(TITAN_CLANG NAMES clang HINTS ${_titan_llvm_hints} NO_DEFAULT_PATH)
if(NOT TITAN_CLANG)
    find_program(TITAN_CLANG NAMES clang-23 clang-22 clang-21 clang REQUIRED)
endif()
set(CMAKE_C_COMPILER "${TITAN_CLANG}" CACHE FILEPATH "" FORCE)

get_filename_component(_titan_llvm_bin "${TITAN_CLANG}" DIRECTORY)
find_program(CMAKE_AR NAMES llvm-ar HINTS "${_titan_llvm_bin}")
find_program(CMAKE_RANLIB NAMES llvm-ranlib HINTS "${_titan_llvm_bin}")
