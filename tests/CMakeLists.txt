function(titan_test name)
    titan_add_executable(test_${name} test_${name}.c)
    target_link_libraries(test_${name} PRIVATE titan_core titan_flecs)
    add_test(NAME ${name} COMMAND test_${name})
endfunction()

titan_test(arena)
titan_test(str)
titan_test(flecs_smoke)
