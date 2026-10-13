# Run with: just fuzz <name> [seconds]. Seed inputs live in fuzz/corpus/<name>/.
function(titan_fuzzer name)
    titan_add_executable(fuzz_${name} fuzz_${name}.c)
    target_link_libraries(fuzz_${name} PRIVATE titan_core titan_flecs)
    target_link_options(fuzz_${name} PRIVATE -fsanitize=fuzzer)
endfunction()

titan_fuzzer(flecs_script)
