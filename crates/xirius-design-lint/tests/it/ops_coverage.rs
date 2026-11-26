use xirius_design_lint::{Rule, lint_source};

#[test]
fn unknown_operations_fail_in_calls_and_indirect_dispatch_tables() {
    for source in [
        "fn click(s: &mut Studio) { s.run_command(\"brand.new\", args); }",
        "const COMMANDS: &[&str] = &[\"note.unsupported\"];",
        "fn dispatch(s: &mut Studio) { s.run_binding(\"track.unknown --on\"); }",
        "fn click(s: &mut Studio) { s.run_command(\"unregistered\", args); }",
        "fn click(s: &mut Studio) { s.run_command(\"Note.Quantize\", args); }",
    ] {
        let violations = lint_source("crates/xirius-ui/src/example.rs", source);
        assert!(
            violations.iter().any(|v| v.rule == Rule::OpsCoverage),
            "{violations:?}"
        );
    }
}

#[test]
fn preference_keys_and_tooltip_identifiers_are_data_not_command_invocations() {
    let source = "fn paint(s: &mut Studio) { let key = \"audio.device\"; let tooltip = \"plugin.open\"; s.run_command(\"prefs.set\", args); }";
    assert!(
        !lint_source("crates/xirius-ui/src/example.rs", source)
            .iter()
            .any(|v| v.rule == Rule::OpsCoverage)
    );
    let source = "fn click(s: &mut Studio) { s.run_command(\"audio.device\", args); }";
    assert!(
        lint_source("crates/xirius-ui/src/example.rs", source)
            .iter()
            .any(|v| v.rule == Rule::OpsCoverage)
    );
}

#[test]
fn registered_parameters_and_test_fixtures_are_allowed() {
    let source = "fn click(s: &mut Studio) { s.run_command(\"note.velocity.shape\", args); } #[cfg(test)] mod tests { const BAD: &str = \"note.fake\"; }";
    let violations = lint_source("crates/xirius-ui/src/example.rs", source);
    assert!(
        !violations.iter().any(|v| v.rule == Rule::OpsCoverage),
        "{violations:?}"
    );
}
