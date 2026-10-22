#[test]
fn readme_marks_vst2_as_unsupported() {
    assert!(include_str!("../../../README.md").contains("VST2 is not supported."));
}
