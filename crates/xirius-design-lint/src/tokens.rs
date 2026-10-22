//! デザイントークンの生成（UI-042）。`crates/xirius-design/tokens.json` が元で、色・余白・角丸・
//! 文字・動き・部品の寸法をここから `crates/xirius-design/src/generated.rs`（GPUI に依存しない
//! Rust）に書き出す。生成した file は直さない。元と合わなければ検査が落ちる。
//!
//! 将来の Web・QML の出力も同じ元から作る（出力の種類を足すだけで、元は 1 つのまま）。

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Map, Value};

/// 元の JSON（ルートからの相対パス）。
pub const SOURCE: &str = "crates/xirius-design/tokens.json";
/// 生成する Rust（ルートからの相対パス）。
pub const OUTPUT: &str = "crates/xirius-design/src/generated.rs";

/// 元の JSON から Rust の text を作る。
pub fn generate(json: &str) -> Result<String, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| format!("JSON が読めない: {e}"))?;
    let root = root.as_object().ok_or("最上位がオブジェクトではない")?;
    let mut out = String::new();
    out.push_str(HEADER);
    palette(&mut out, array(root, "colors")?)?;
    scale(&mut out, root, "space", "Space", "px", "f32")?;
    scale(&mut out, root, "radius", "Radius", "px", "f32")?;
    scale(&mut out, root, "font_size", "FontSize", "px", "f32")?;
    scale(&mut out, root, "line_height", "LineHeight", "ratio", "f32")?;
    scale(&mut out, root, "weight", "Weight", "value", "u16")?;
    scale(&mut out, root, "duration_ms", "Duration", "ms", "u32")?;
    family(&mut out, object(root, "font_family")?)?;
    let ai = number(root, "ai_loop_ms")?;
    write!(
        out,
        "\n/// AI が動いている印の 1 周（ミリ秒）。\npub const AI_LOOP_MS: u32 = {};\n",
        whole(ai, "ai_loop_ms")?
    )
    .map_err(fmt)?;
    easing(&mut out, array(root, "easing")?)?;
    layout(&mut out, array(root, "layout")?)?;
    Ok(out)
}

/// 元と生成した file が合っているか。合わなければ、何を直すかを返す。
pub fn check(root: &Path) -> Result<(), String> {
    let expected = expected(root)?;
    let found = std::fs::read_to_string(root.join(OUTPUT))
        .map_err(|e| format!("{OUTPUT} が読めない: {e}"))?;
    if found.replace("\r\n", "\n") == expected {
        Ok(())
    } else {
        Err(format!(
            "{OUTPUT} が {SOURCE} と合わない（`cargo run -p xirius-design-lint -- tokens --write` で作り直す）。{}",
            first_difference(&found, &expected)
        ))
    }
}

/// 生成した file を書き直す。変わったかを返す。
pub fn write(root: &Path) -> Result<bool, String> {
    let expected = expected(root)?;
    let path = root.join(OUTPUT);
    if std::fs::read_to_string(&path).is_ok_and(|found| found.replace("\r\n", "\n") == expected) {
        return Ok(false);
    }
    std::fs::write(&path, expected).map_err(|e| format!("{OUTPUT} が書けない: {e}"))?;
    Ok(true)
}

fn expected(root: &Path) -> Result<String, String> {
    let json = std::fs::read_to_string(root.join(SOURCE))
        .map_err(|e| format!("{SOURCE} が読めない: {e}"))?;
    generate(&json).map_err(|e| format!("{SOURCE}: {e}"))
}

/// 最初に違う行（どこが食い違うかの手がかり）。
fn first_difference(found: &str, expected: &str) -> String {
    let found = found.replace("\r\n", "\n");
    for (i, (a, b)) in found.lines().zip(expected.lines()).enumerate() {
        if a != b {
            return format!("{} 行目: 今は `{a}`、元からは `{b}`", i + 1);
        }
    }
    format!(
        "行の数が違う（今 {}、元から {}）",
        found.lines().count(),
        expected.lines().count()
    )
}

const HEADER: &str = "\
// 生成した file。直さない。元は tokens.json で、作り直すのは
// `cargo run -p xirius-design-lint -- tokens --write`（元と合わなければ検査が落ちる）。

";

fn fmt(_: std::fmt::Error) -> String {
    "書き出せない".to_string()
}

fn array<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a Vec<Value>, String> {
    m.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("`{key}` が配列ではない"))
}

fn object<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a Map<String, Value>, String> {
    m.get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("`{key}` がオブジェクトではない"))
}

fn number(m: &Map<String, Value>, key: &str) -> Result<f64, String> {
    m.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("`{key}` が数ではない"))
}

fn text<'a>(m: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a str, String> {
    m.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{at}: `{key}` が文字列ではない"))
}

fn entry<'a>(v: &'a Value, at: &str) -> Result<&'a Map<String, Value>, String> {
    v.as_object()
        .ok_or_else(|| format!("{at}: 要素がオブジェクトではない"))
}

/// 名前（Rust の識別子として使える形）。
fn ident(s: &str, at: &str) -> Result<(), String> {
    let ok = s
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!("{at}: `{s}` は識別子にできない"))
    }
}

fn whole(n: f64, at: &str) -> Result<u64, String> {
    if n >= 0.0 && n.fract() == 0.0 {
        Ok(n as u64)
    } else {
        Err(format!("{at}: 整数ではない（{n}）"))
    }
}

/// 数の書き方（Rust の浮動小数点の定数として読める形）。
fn float(n: f64) -> String {
    format!("{n:?}")
}

/// `#rrggbb` か `#rrggbbaa` を、パレットの `c(..)` / `ca(.., a)` にする。
fn color(s: &str, at: &str) -> Result<String, String> {
    let digits = s.strip_prefix('#').unwrap_or("");
    if !(digits.len() == 6 || digits.len() == 8) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{at}: `{s}` は #rrggbb か #rrggbbaa ではない"));
    }
    let rgb = digits[..6].to_ascii_lowercase();
    if digits.len() == 8 {
        let alpha = u8::from_str_radix(&digits[6..], 16).map_err(|e| format!("{at}: {e}"))?;
        Ok(format!("ca(0x{rgb}, {alpha})"))
    } else {
        Ok(format!("c(0x{rgb})"))
    }
}

fn comment(
    out: &mut String,
    indent: &str,
    e: &Map<String, Value>,
    at: &str,
) -> Result<bool, String> {
    let Some(lines) = e.get("comment") else {
        return Ok(false);
    };
    let lines = lines
        .as_array()
        .ok_or_else(|| format!("{at}: `comment` が配列ではない"))?;
    for l in lines {
        let l = l
            .as_str()
            .ok_or_else(|| format!("{at}: `comment` の行が文字列ではない"))?;
        writeln!(out, "{indent}// {l}").map_err(fmt)?;
    }
    Ok(true)
}

fn doc(out: &mut String, indent: &str, e: &Map<String, Value>, at: &str) -> Result<(), String> {
    if e.contains_key("doc") {
        writeln!(out, "{indent}/// {}", text(e, "doc", at)?).map_err(fmt)?;
    }
    Ok(())
}

fn palette(out: &mut String, colors: &[Value]) -> Result<(), String> {
    out.push_str("/// `palette!` に渡す、全トークンの色（ダーク・ライト）。\nmacro_rules! palette_tokens {\n    ($m:ident) => {\n        $m! {\n");
    let mut seen = std::collections::BTreeSet::new();
    for (i, v) in colors.iter().enumerate() {
        let at = format!("colors[{i}]");
        let e = entry(v, &at)?;
        if comment(out, "            ", e, &at)? {
            continue;
        }
        let name = text(e, "name", &at)?;
        ident(name, &at)?;
        if !seen.insert(name) {
            return Err(format!("{at}: `{name}` が 2 つある"));
        }
        doc(out, "            ", e, &at)?;
        let dark = color(text(e, "dark", &at)?, &format!("{at} {name} dark"))?;
        let light = color(text(e, "light", &at)?, &format!("{at} {name} light"))?;
        writeln!(out, "            {name}: {dark}, {light};").map_err(fmt)?;
    }
    out.push_str("        }\n    };\n}\n");
    Ok(())
}

/// 列挙型の値の表（`Space::X1` → 4.0 など）。
fn scale(
    out: &mut String,
    root: &Map<String, Value>,
    key: &str,
    ty: &str,
    method: &str,
    ret: &str,
) -> Result<(), String> {
    let table = object(root, key)?;
    write!(
        out,
        "\nimpl crate::{ty} {{\n    pub const fn {method}(self) -> {ret} {{\n        match self {{\n"
    )
    .map_err(fmt)?;
    for (name, v) in by_name(table) {
        let at = format!("{key}.{name}");
        ident(name, &at)?;
        let n = v.as_f64().ok_or_else(|| format!("{at}: 数ではない"))?;
        let n = if ret == "f32" {
            float(n)
        } else {
            whole(n, &at)?.to_string()
        };
        writeln!(out, "            crate::{ty}::{name} => {n},").map_err(fmt)?;
    }
    out.push_str("        }\n    }\n}\n");
    Ok(())
}

/// The entries of a table by name, so the output does not hang on how serde_json keeps a map's
/// order (its `preserve_order` feature comes and goes with other crates of the workspace).
fn by_name(m: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| a.0.cmp(b.0));
    v
}

fn family(out: &mut String, names: &Map<String, Value>) -> Result<(), String> {
    out.push_str(
        "\nimpl crate::Font {\n    /// OS に登録するときの family 名。\n    pub const fn family(self) -> &'static str {\n        match self {\n",
    );
    for (name, v) in by_name(names) {
        let at = format!("font_family.{name}");
        ident(name, &at)?;
        let family = v
            .as_str()
            .filter(|s| !s.contains(['"', '\\']))
            .ok_or_else(|| format!("{at}: 文字列でない（\" と \\ は使えない）"))?;
        writeln!(out, "            crate::Font::{name} => \"{family}\",").map_err(fmt)?;
    }
    out.push_str("        }\n    }\n}\n");
    Ok(())
}

fn easing(out: &mut String, curves: &[Value]) -> Result<(), String> {
    out.push_str("\nimpl crate::Easing {\n");
    for (i, v) in curves.iter().enumerate() {
        let at = format!("easing[{i}]");
        let e = entry(v, &at)?;
        let name = text(e, "name", &at)?;
        ident(name, &at)?;
        doc(out, "    ", e, &at)?;
        let curve = e
            .get("curve")
            .and_then(Value::as_array)
            .filter(|c| c.len() == 4)
            .ok_or_else(|| format!("{at}: `curve` が 4 つの数ではない"))?;
        let curve: Vec<String> = curve
            .iter()
            .map(|n| {
                n.as_f64()
                    .map(float)
                    .ok_or_else(|| format!("{at}: `curve` に数でない値がある"))
            })
            .collect::<Result<_, _>>()?;
        writeln!(
            out,
            "    pub const {name}: crate::Easing = crate::Easing([{}]);",
            curve.join(", ")
        )
        .map_err(fmt)?;
    }
    out.push_str("}\n");
    Ok(())
}

fn layout(out: &mut String, items: &[Value]) -> Result<(), String> {
    out.push_str("\nimpl crate::Layout {\n");
    let mut seen = std::collections::BTreeSet::new();
    for (i, v) in items.iter().enumerate() {
        let at = format!("layout[{i}]");
        let e = entry(v, &at)?;
        if comment(out, "    ", e, &at)? {
            continue;
        }
        let name = text(e, "name", &at)?;
        ident(name, &at)?;
        if !seen.insert(name) {
            return Err(format!("{at}: `{name}` が 2 つある"));
        }
        doc(out, "    ", e, &at)?;
        let n = number(e, "value").map_err(|m| format!("{at} {name}: {m}"))?;
        let (ty, n) = match text(e, "type", &at)? {
            "f32" => ("f32", float(n)),
            "usize" => ("usize", whole(n, &at)?.to_string()),
            other => return Err(format!("{at} {name}: 型 `{other}` は f32 か usize")),
        };
        writeln!(out, "    pub const {name}: {ty} = {n};").map_err(fmt)?;
    }
    out.push_str("}\n");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = r##"{
      "colors": [
        {"comment": ["---- 面 ----"]},
        {"name": "surface_0", "dark": "#121318", "light": "#ECEEF4", "doc": "地"},
        {"name": "surface_veil", "dark": "#0000008c", "light": "#15161c52"}
      ],
      "space": {"X1": 4.0, "X2": 8.0},
      "radius": {"S": 8.0},
      "font_size": {"Body": 14.0},
      "line_height": {"Body": 1.6},
      "weight": {"Regular": 400},
      "duration_ms": {"Fast": 120},
      "font_family": {"Inter": "Inter"},
      "ai_loop_ms": 1000,
      "easing": [{"name": "OUT", "curve": [0.2, 0.7, 0.2, 1.0], "doc": "出入り"}],
      "layout": [
        {"comment": ["---- 上 ----"]},
        {"name": "TOP_BAR_H", "type": "f32", "value": 40.0, "doc": "高さ"},
        {"name": "DEPTH", "type": "usize", "value": 3}
      ]
    }"##;

    fn small() -> String {
        generate(SMALL).unwrap()
    }

    #[test]
    fn colors_become_palette_entries_with_alpha_split_out() {
        let out = small();
        assert!(out.contains("            // ---- 面 ----\n"), "{out}");
        assert!(out.contains("            /// 地\n"), "{out}");
        assert!(
            out.contains("surface_0: c(0x121318), c(0xeceef4);"),
            "{out}"
        );
        assert!(
            out.contains("surface_veil: ca(0x000000, 140), ca(0x15161c, 82);"),
            "{out}"
        );
    }

    #[test]
    fn scales_become_const_matches() {
        let out = small();
        assert!(out.contains("impl crate::Space {"), "{out}");
        assert!(out.contains("crate::Space::X2 => 8.0,"), "{out}");
        assert!(out.contains("crate::LineHeight::Body => 1.6,"), "{out}");
        assert!(out.contains("crate::Weight::Regular => 400,"), "{out}");
        assert!(out.contains("crate::Duration::Fast => 120,"), "{out}");
        assert!(out.contains("crate::Font::Inter => \"Inter\","), "{out}");
        assert!(out.contains("pub const AI_LOOP_MS: u32 = 1000;"), "{out}");
    }

    #[test]
    fn layout_and_easing_keep_their_docs() {
        let out = small();
        assert!(
            out.contains("    /// 高さ\n    pub const TOP_BAR_H: f32 = 40.0;"),
            "{out}"
        );
        assert!(out.contains("pub const DEPTH: usize = 3;"), "{out}");
        assert!(
            out.contains("pub const OUT: crate::Easing = crate::Easing([0.2, 0.7, 0.2, 1.0]);"),
            "{out}"
        );
    }

    #[test]
    fn the_same_source_gives_the_same_text() {
        assert_eq!(small(), small());
    }

    #[test]
    fn a_wrong_source_is_named() {
        for (from, to, hint) in [
            ("#121318", "121318", "#rrggbb"),
            ("#121318", "#12131", "#rrggbb"),
            ("\"surface_0\"", "\"surface 0\"", "識別子"),
            ("\"value\": 3", "\"value\": 3.5", "整数"),
            ("\"f32\"", "\"f64\"", "f64"),
            (
                "\"ai_loop_ms\": 1000",
                "\"ai_loop_ms\": \"x\"",
                "ai_loop_ms",
            ),
        ] {
            let err = generate(&SMALL.replacen(from, to, 1)).unwrap_err();
            assert!(err.contains(hint), "{from} → {to}: {err}");
        }
        let twice = SMALL.replacen("\"surface_veil\"", "\"surface_0\"", 1);
        assert!(generate(&twice).unwrap_err().contains("2 つある"));
    }

    #[test]
    fn a_difference_names_the_first_line() {
        let a = "a\nb\nc\n";
        assert!(first_difference(a, "a\nx\nc\n").contains("2 行目"));
        assert!(first_difference(a, "a\nb\n").contains("行の数"));
    }
}
