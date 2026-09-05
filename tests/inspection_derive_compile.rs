#[test]
fn boolean_inspection_attributes_compile_or_fail_downstream() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/ui/inspection_bool/pass.rs");
    tests.compile_fail("tests/ui/inspection_bool/bounds.rs");
}
