# Web toolchain: wasm32 without emscripten. See docs/adr/0007-web-without-emscripten.md.
#
# C code is compiled against wasi-libc and linked statically, but the module imports
# nothing from WASI: src/platform/web/wasi_shim.c implements the few __wasi_* syscall
# wrappers libc needs on top of a tiny host interface (env.titan_*), provided by our JS.
#
#   WASI_SYSROOT   wasi-libc sysroot      (default: Homebrew wasi-libc)
#   WASI_RUNTIMES  clang resource dir with wasm32 compiler-rt builtins (default: Homebrew wasi-runtimes)
#
# With wasi-sdk (as in CI): LLVM_ROOT=<sdk>, WASI_SYSROOT=<sdk>/share/wasi-sysroot,
# WASI_RUNTIMES=<sdk>/lib/clang/<version>.

set(CMAKE_SYSTEM_NAME WASI)
set(CMAKE_SYSTEM_PROCESSOR wasm32)

include("${CMAKE_CURRENT_LIST_DIR}/clang.cmake")

set(_titan_target wasm32-wasip1)
set(CMAKE_C_COMPILER_TARGET ${_titan_target})

if(DEFINED ENV{WASI_SYSROOT})
    set(TITAN_WASI_SYSROOT "$ENV{WASI_SYSROOT}")
else()
    set(TITAN_WASI_SYSROOT /opt/homebrew/opt/wasi-libc/share/wasi-sysroot)
endif()
if(DEFINED ENV{WASI_RUNTIMES})
    set(TITAN_WASI_RUNTIMES "$ENV{WASI_RUNTIMES}")
else()
    set(TITAN_WASI_RUNTIMES /opt/homebrew/share/wasi-runtimes)
endif()
foreach(_dir "${TITAN_WASI_SYSROOT}/include/${_titan_target}" "${TITAN_WASI_RUNTIMES}/lib/wasm32-unknown-wasip1")
    if(NOT EXISTS "${_dir}")
        message(FATAL_ERROR "Missing ${_dir}: install wasi-libc and wasi-runtimes (brew install wasi-libc wasi-runtimes) or set WASI_SYSROOT / WASI_RUNTIMES")
    endif()
endforeach()

set(CMAKE_SYSROOT "${TITAN_WASI_SYSROOT}")
find_program(TITAN_WASM_LD NAMES wasm-ld HINTS "${_titan_llvm_bin}" /opt/homebrew/opt/lld/bin REQUIRED)
set(CMAKE_C_FLAGS_INIT "-resource-dir=${TITAN_WASI_RUNTIMES}")
# 1 MiB stack (wasm-ld defaults to 64 KiB, too small for flecs). --stack-first puts the stack at
# the bottom of memory so an overflow traps instead of silently corrupting static data.
set(CMAKE_EXE_LINKER_FLAGS_INIT "-resource-dir=${TITAN_WASI_RUNTIMES} -fuse-ld=${TITAN_WASM_LD} -Wl,-z,stack-size=1048576 -Wl,--stack-first")

# Tests run in Node (same V8 engine as Chrome) through our JS host.
find_program(TITAN_NODE NAMES node REQUIRED)
set(CMAKE_CROSSCOMPILING_EMULATOR "${TITAN_NODE};${CMAKE_CURRENT_LIST_DIR}/../tools/wasm_run.mjs")

set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
