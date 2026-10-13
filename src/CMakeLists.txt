add_library(titan_core STATIC
    core/arena.c
    core/str.c
    core/version.c)
target_include_directories(titan_core PUBLIC "${PROJECT_SOURCE_DIR}/include")
target_compile_definitions(titan_core PRIVATE TITAN_VERSION_STRING="${PROJECT_VERSION}")
titan_target_defaults(titan_core)

if(CMAKE_SYSTEM_NAME STREQUAL "WASI")
    # Object library: these definitions must always win over wasi-libc's syscall wrappers.
    add_library(titan_platform_web OBJECT platform/web/wasi_shim.c)
    titan_target_defaults(titan_platform_web)
endif()

# Every Titan executable (apps, tests, fuzzers) goes through this so platform code is linked in.
function(titan_add_executable name)
    add_executable(${name} ${ARGN})
    titan_target_defaults(${name})
    if(TARGET titan_platform_web)
        target_sources(${name} PRIVATE $<TARGET_OBJECTS:titan_platform_web>)
    endif()
endfunction()
