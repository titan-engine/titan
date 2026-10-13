# Compiler settings for Titan's own code. Third-party code is built separately with
# warnings off (see third_party/CMakeLists.txt), but sanitizers apply to everything.

option(TITAN_WERROR "Treat warnings as errors" ON)
set(TITAN_SANITIZE "" CACHE STRING "Comma-separated -fsanitize= list (address,undefined / thread / memory / fuzzer-no-link,...)")

if(TITAN_SANITIZE)
    add_compile_options(-fsanitize=${TITAN_SANITIZE} -fno-sanitize-recover=all -fno-omit-frame-pointer)
    add_link_options(-fsanitize=${TITAN_SANITIZE})
    if(TITAN_SANITIZE MATCHES "memory")
        add_compile_options(-fsanitize-memory-track-origins)
    endif()
endif()

set(TITAN_WARNINGS
    -Wall -Wextra -Wpedantic
    -Wshadow -Wconversion -Wsign-conversion -Wdouble-promotion
    -Wvla -Wimplicit-fallthrough -Wundef -Wcast-qual -Wformat=2
    -Wmissing-prototypes -Wstrict-prototypes -Wmissing-variable-declarations
    -Wnewline-eof -Wcomma)

# Apply to every target that contains Titan code.
function(titan_target_defaults target)
    set_target_properties(${target} PROPERTIES C_STANDARD 23 C_STANDARD_REQUIRED ON C_EXTENSIONS OFF)
    target_compile_options(${target} PRIVATE ${TITAN_WARNINGS})
    if(TITAN_WERROR)
        target_compile_options(${target} PRIVATE -Werror)
    endif()
endfunction()
