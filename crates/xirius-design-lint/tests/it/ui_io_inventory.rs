use std::path::PathBuf;

#[test]
fn reviewed_ui_boundaries_include_suppressed_calls_and_cannot_grow_silently() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let actual = xirius_design_lint::ui_io_inventory(&root).unwrap();
    let reviewed = std::fs::read_to_string(root.join("docs/ui-io-audit.tsv")).unwrap();
    assert_eq!(
        actual, reviewed,
        "Review the new boundary, update docs/ui-io.md and regenerate --ui-io-inventory"
    );
}
