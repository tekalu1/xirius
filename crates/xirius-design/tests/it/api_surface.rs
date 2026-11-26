//! 公開の口が、デザインの規則を破れない形のままか確かめる。
//! ソースの文字列を読む検査と、型の検査（`compile_fail`）の組。

use std::fs;
use std::path::PathBuf;

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(src_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            out.push((name, fs::read_to_string(&path).unwrap()));
        }
    }
    out
}

/// コードの行（`//` で始まるコメントの行は除く）。
fn code_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|l| !l.trim_start().starts_with("//"))
}

#[test]
fn gradient_types_live_only_in_the_meter_module() {
    for (name, text) in sources() {
        if name == "meter.rs" {
            continue;
        }
        for line in code_lines(&text) {
            let lower = line.to_lowercase();
            // lib.rs の再公開（`pub use meter::{...}`）だけは meter の物を名指す。
            if name == "lib.rs" && lower.contains("pub use meter") {
                continue;
            }
            assert!(
                !lower.contains("gradient"),
                "{name}: グラデーションはメーター（meter.rs）だけ: {line}"
            );
        }
    }
}

#[test]
fn no_border_or_line_width_api() {
    for (name, text) in sources() {
        for line in code_lines(&text) {
            let lower = line.to_lowercase();
            for banned in [
                "border",
                "stroke_width",
                "line_width",
                "hairline",
                "outline",
            ] {
                assert!(
                    !lower.contains(banned),
                    "{name}: 線の API は作らない: {line}"
                );
            }
        }
    }
}

#[test]
fn no_weak_ink_or_tint_in_code() {
    for (name, text) in sources() {
        for line in code_lines(&text) {
            let lower = line.to_lowercase();
            for banned in ["ink_weak", "weak", "tint", "faint", "muted"] {
                assert!(
                    !lower.contains(banned),
                    "{name}: 薄い文字・色の面は作らない: {line}"
                );
            }
        }
    }
}

#[test]
fn only_the_two_fonts_are_named() {
    for (name, text) in sources() {
        if name != "font.rs" {
            continue;
        }
        for line in code_lines(&text) {
            for banned in [
                "Roboto",
                "Helvetica",
                "Arial",
                "Segoe",
                "San Francisco",
                "Hiragino",
                "Meiryo",
                "Yu Gothic",
            ] {
                assert!(
                    !line.contains(banned),
                    "{name}: 使う書体は 2 つだけ: {line}"
                );
            }
        }
    }
}

#[test]
fn palette_is_not_reachable_from_outside() {
    // `Palette` は pub(crate)。外から名前が引けないことは、ここでは文字列で確かめる。
    let lib = fs::read_to_string(src_dir().join("lib.rs")).unwrap();
    assert!(!lib.contains("pub use palette::Palette"));
    let palette = fs::read_to_string(src_dir().join("palette.rs")).unwrap();
    assert!(palette.contains("pub(crate) struct Palette"));
}
