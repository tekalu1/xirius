//! Every window test starts the app through `tests/support`'s `app`, with a profile, a temporary
//! folder and plugin folders of its own: run on a person's PC, it writes nothing of theirs (the
//! browser's `Xirius\browser.json.lock`, `Xirius\Crashes`) and loads none of their plugins. The
//! window tests run off CI (`scripts/ci_tests.py`), where nothing would notice; this check, read
//! from the sources, runs on CI.

use std::path::Path;

/// The other processes a window test starts, by file: how many, and why it may.
const OTHER: [(&str, usize, &str); 3] = [
    (
        "crash_entry.rs",
        1,
        "the test's own program, as the process that crashes, writing to its own crash folder",
    ),
    (
        "independent_verify_window.rs",
        1,
        "python, whose script gives the app it starts folders of its own",
    ),
    (
        "otherproc_window.rs",
        1,
        "cargo, building the `xirius` program",
    ),
];

/// The window tests among the test targets, as `scripts/ci_tests.py` (`opens_windows`) names them.
fn opens_windows(stem: &str) -> bool {
    stem.ends_with("_window") || stem.ends_with("_windows") || stem == "crash_entry"
}

#[test]
fn window_tests_start_the_app_with_folders_of_their_own() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ci = std::fs::read_to_string(root.join("../../scripts/ci_tests.py")).unwrap();
    assert!(
        ci.contains(r#""xirius-ui": {"crash_entry"},"#),
        "ci_tests.py names other window tests of xirius-ui: name them in `opens_windows` here"
    );
    let mut seen = 0;
    for entry in std::fs::read_dir(root.join("tests")).unwrap().flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().is_none_or(|x| x != "rs") || !opens_windows(stem) {
            continue;
        }
        seen += 1;
        let name = format!("{stem}.rs");
        let text = std::fs::read_to_string(&path).unwrap();
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let allowed = OTHER.iter().find(|o| o.0 == name).map_or(0, |o| o.1);
        assert_eq!(
            code.matches("Command::new(").count(),
            allowed,
            "{name} starts a process of its own: start the app with `support::app(root)` \
             (tests/support/mod.rs), or name the process and why in OTHER here"
        );
        assert!(
            code.contains("support::app("),
            "{name} does not start the app with `support::app(root)`"
        );
    }
    assert!(seen > 20, "{seen} window tests found");
    for (name, _, _) in OTHER {
        assert!(root.join("tests").join(name).is_file(), "{name} is gone");
    }
}
