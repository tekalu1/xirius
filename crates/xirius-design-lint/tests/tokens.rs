//! UI-042: the generated Rust of the design tokens is the one `tokens.json` gives; a change of
//! either alone fails the check.

use std::fs;
use std::path::{Path, PathBuf};

use xirius_design_lint::tokens::{self, OUTPUT, SOURCE};

/// The repository root (two levels above this crate).
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A copy of the two files in a fresh directory, to change. The directory goes when dropped
/// (also when the test fails).
fn copy_of_the_tokens(name: &str) -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix(&format!("xirius-tokens-{name}-"))
        .tempdir()
        .unwrap();
    for rel in [SOURCE, OUTPUT] {
        let to = dir.path().join(rel);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(repo().join(rel), to).unwrap();
    }
    dir
}

#[test]
fn the_generated_rust_of_the_repository_is_current() {
    tokens::check(&repo()).unwrap();
}

#[test]
fn a_changed_color_in_the_json_fails_the_check_until_it_is_generated_again() {
    let tmp = copy_of_the_tokens("json");
    let dir = tmp.path();
    let path = dir.join(SOURCE);
    let json = fs::read_to_string(&path).unwrap();
    assert!(json.contains("\"#1b1c23\""));
    fs::write(&path, json.replacen("\"#1b1c23\"", "\"#1b1c24\"", 1)).unwrap();
    let err = tokens::check(dir).unwrap_err();
    assert!(
        err.contains("合わない") && err.contains("0x1b1c23"),
        "{err}"
    );
    assert!(tokens::write(dir).unwrap());
    tokens::check(dir).unwrap();
    assert!(
        fs::read_to_string(dir.join(OUTPUT))
            .unwrap()
            .contains("c(0x1b1c24)")
    );
    assert!(!tokens::write(dir).unwrap());
}

#[test]
fn a_hand_edit_of_the_generated_rust_fails_the_check() {
    let tmp = copy_of_the_tokens("rust");
    let dir = tmp.path();
    let path = dir.join(OUTPUT);
    let rust = fs::read_to_string(&path).unwrap();
    fs::write(&path, rust.replacen("=> 4.0,", "=> 5.0,", 1)).unwrap();
    assert!(tokens::check(dir).is_err());
}

#[test]
fn line_endings_do_not_matter() {
    let tmp = copy_of_the_tokens("crlf");
    let dir = tmp.path();
    let path = dir.join(OUTPUT);
    let rust = fs::read_to_string(&path).unwrap();
    fs::write(&path, rust.replace('\n', "\r\n")).unwrap();
    tokens::check(dir).unwrap();
}
