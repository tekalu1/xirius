#[test]
fn command_reference_matches_the_complete_registry() {
    let expected = xirius_core::commands::docs::markdown(&xirius_session::commands::registry());
    let actual = include_str!("../../../docs/commands.md").replace("\r\n", "\n");
    let line = actual
        .lines()
        .zip(expected.lines())
        .position(|(a, b)| a != b)
        .map(|line| line + 1);
    assert!(
        actual == expected,
        "run xirius --commands-doc docs/commands.md (first changed line: {line:?})"
    );
}

#[test]
fn stale_generated_document_is_rejected_by_the_command_line_check() {
    let path_tmp = tempfile::Builder::new()
        .prefix("xirius-command-docs-")
        .tempdir()
        .unwrap();
    let path = path_tmp.path().join("command-docs.md");
    let executable = env!("CARGO_BIN_EXE_xirius");
    assert!(
        std::process::Command::new(executable)
            .arg("--commands-doc")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new(executable)
            .args(["--commands-doc", "--check"])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\n変更\n");
    std::fs::write(&path, text).unwrap();
    assert!(
        !std::process::Command::new(executable)
            .args(["--commands-doc", "--check"])
            .arg(&path)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::remove_file(path).unwrap();
}
