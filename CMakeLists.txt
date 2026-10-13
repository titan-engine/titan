cmake_minimum_required(VERSION 3.28)
project(titan VERSION 0.1.0 LANGUAGES C)

if(NOT CMAKE_C_COMPILER_ID STREQUAL "Clang")
    message(FATAL_ERROR "Titan requires LLVM clang, found ${CMAKE_C_COMPILER_ID}. "
                        "Configure through a preset (just build) or see docs/adr/0002-c23-and-clang-only.md")
endif()
if(CMAKE_C_COMPILER_VERSION VERSION_LESS 21)
    message(FATAL_ERROR "Titan requires clang >= 21, found ${CMAKE_C_COMPILER_VERSION}")
endif()

if(CMAKE_SYSTEM_NAME STREQUAL "WASI")
    set(CMAKE_EXECUTABLE_SUFFIX ".wasm")
endif()

find_program(TITAN_CCACHE ccache)
if(TITAN_CCACHE)
    set(CMAKE_C_COMPILER_LAUNCHER "${TITAN_CCACHE}")
endif()

include(cmake/titan_defaults.cmake)

option(TITAN_BUILD_TESTS "Build unit tests" ON)
option(TITAN_BUILD_FUZZERS "Build libFuzzer targets (needs TITAN_SANITIZE=fuzzer-no-link,...)" OFF)

add_subdirectory(third_party)
add_subdirectory(src)

if(TITAN_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()
if(TITAN_BUILD_FUZZERS)
    add_subdirectory(fuzz)
endif()
