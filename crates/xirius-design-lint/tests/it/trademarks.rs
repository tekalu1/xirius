//! LIC-009: the name, the logo and the product wording do not use VST or ASIO as a name.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn names_a_steinberg_mark(text: &str) -> bool {
    let up = text.to_uppercase();
    up.contains("VST") || up.contains("ASIO")
}

#[test]
fn trademarks_md_covers_the_name_the_logo_and_the_two_marks() {
    let doc = fs::read_to_string(root().join("TRADEMARKS.md")).expect("TRADEMARKS.md");
    for needle in ["Xirius", "logo", "VST", "ASIO", "Steinberg", "fork"] {
        assert!(doc.contains(needle), "TRADEMARKS.md lacks {needle}");
    }
}

#[test]
fn readme_points_at_trademarks_md() {
    let readme = fs::read_to_string(root().join("README.md")).expect("README.md");
    assert!(readme.contains("TRADEMARKS.md"));
}

#[test]
fn the_logo_files_do_not_say_vst_or_asio() {
    let dir = root().join("assets/brand");
    let mut seen = 0;
    for entry in fs::read_dir(&dir).expect("assets/brand") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "svg") {
            let text = fs::read_to_string(&path).expect("svg");
            assert!(!names_a_steinberg_mark(&text), "{}", path.display());
            seen += 1;
        }
    }
    assert!(seen >= 6, "the logo files are where they were: {seen}");
}

#[test]
fn the_product_name_in_the_window_the_about_screen_and_the_package_has_no_mark() {
    let app = fs::read_to_string(root().join("crates/xirius-ui/src/app.rs")).expect("app.rs");
    let title = app
        .lines()
        .find(|l| l.starts_with("pub const WINDOW_TITLE"))
        .expect("WINDOW_TITLE");
    assert!(!names_a_steinberg_mark(title), "{title}");

    let about = fs::read_to_string(root().join("crates/xirius-ui/src/about.rs")).expect("about");
    // Above the tests: what the screen says.
    let shown = about.split("#[cfg(test)]").next().expect("about source");
    assert!(!names_a_steinberg_mark(shown), "the About screen's wording");

    let notice = fs::read_to_string(root().join("NOTICE")).expect("NOTICE");
    assert!(!names_a_steinberg_mark(&notice), "NOTICE");

    // The workspace's product binaries and packages are named xirius*, never after a mark
    // (the plugin-format crates may say vst3: that names a format, not the product).
    let manifest = fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");
    assert!(!manifest.to_uppercase().contains("ASIO"), "Cargo.toml");
}
