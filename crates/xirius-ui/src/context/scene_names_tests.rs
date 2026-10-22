//! The names of the rows across every scene (CTX-008, ARC-027): one operation has one name wherever
//! it is a row, a row that opens a dialog or asks for a value ends with "…" and no other does, and
//! the words the matrix and the command table disagreed on are the table's. The product's own
//! terms are held by `xirius-design-lint` (wording).

use super::names_tests::{SOURCES, literals};
use super::tests::{BAR, labels, lane_at, studio};
use crate::arrange::probe::{clip_mid, folder, row_mid, track};
use crate::tree::RowKind;

/// Every source that puts words on a menu row.
fn all_sources() -> Vec<(&'static str, &'static str)> {
    let mut all: Vec<_> = SOURCES.to_vec();
    all.push(("browser.rs", include_str!("browser.rs")));
    all.push(("export.rs", include_str!("export.rs")));
    all.push((
        "../instrument/menu.rs",
        include_str!("../instrument/menu.rs"),
    ));
    all
}

/// The code of a source on one line: comments out, runs of white space made one space.
fn code(src: &str) -> String {
    let lines: Vec<&str> = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect();
    lines
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first string literal after each `needle` (the label of the row), with the code that follows.
fn calls<'a>(code: &'a str, needle: &str) -> Vec<(String, &'a str)> {
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(i) = rest.find(needle) {
        let tail = &rest[i + needle.len()..];
        rest = tail;
        if let Some(t) = tail.trim_start().strip_prefix('"')
            && let Some(end) = t.find('"')
        {
            out.push((t[..end].to_string(), &t[end + 1..]));
        }
    }
    out
}

/// The `Act::` the row runs: the first one in the next call arguments.
fn act_of(after: &str) -> Option<String> {
    let near: String = after.chars().take(160).collect();
    let i = near.find("Act::")?;
    Some(
        near[i + 5..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect(),
    )
}

/// The rows that open a dialog, a panel or a picker: the others act on the spot.
const DIALOG_ACTS: &[&str] = &[
    "Stems",
    "FolderStem",
    "ExportLoop",
    "ExportSpan",
    "PickInsert",
    "PointValue",
    "MidiExport",
];

/// Rows that end with "…" and are neither an `ask_number` nor one of the acts above: the four
/// values of a note's panel, and the picker of the instrument's menu.
const OTHER_DOTS: &[&str] = &["位置…", "長さ…", "ピッチ…", "ベロシティ…", "差し替える…"];

#[test]
fn only_a_row_that_asks_or_opens_a_dialog_ends_with_the_dots() {
    let mut allowed: Vec<String> = OTHER_DOTS.iter().map(|s| (*s).to_string()).collect();
    for (file, src) in all_sources() {
        let c = code(src);
        for (label, _) in calls(&c, "Item::ask_number(") {
            assert!(label.ends_with('…'), "{file}: 「{label}」は値を聞く行");
            allowed.push(label);
        }
        for (label, _) in calls(&c, "Item::sub(") {
            assert!(!label.ends_with('…'), "{file}: 「{label}」はサブメニュー");
        }
        for (label, after) in calls(&c, "Item::act(") {
            let Some(act) = act_of(after) else { continue };
            let opens = DIALOG_ACTS.contains(&act.as_str());
            assert_eq!(
                label.ends_with('…'),
                opens,
                "{file}: 「{label}」（Act::{act}）"
            );
            if opens {
                allowed.push(label);
            }
        }
    }
    for (file, src) in all_sources() {
        for (line, s) in literals(src) {
            if s.ends_with('…') {
                assert!(
                    allowed.contains(&s),
                    "{file}:{line}: 「{s}」の … の理由が無い"
                );
            }
        }
    }
}

/// A word a row used to say, and the one it says now.
const OLD_WORDS: &[(&str, &str)] = &[
    ("足す", "追加"),
    ("消す", "削除"),
    ("外す", "削除"),
    ("変える", "変更"),
    ("点を", "ポイントを"),
    ("点の", "ポイントの"),
    ("点まで", "ポイントまで"),
    ("書き出し…", "書き出す…"),
    ("選ぶ", "選択"),
];

/// Rows that keep a word above because the word is the thing: a number is picked, not selected.
const KEPT: &[&str] = &["番号で選ぶ", "パラメーターを選ぶ"];

#[test]
fn no_row_says_a_word_the_other_scenes_do_not() {
    for (file, src) in all_sources() {
        for (line, s) in literals(src) {
            if KEPT.contains(&s.as_str()) {
                continue;
            }
            for (old, new) in OLD_WORDS {
                assert!(
                    !s.contains(old),
                    "{file}:{line}: 「{s}」の「{old}」は「{new}」"
                );
            }
        }
    }
}

/// The commands whose rows say "外す": the thing leaves the list and nothing is deleted.
const TAKE_OFF: &[&str] = &[
    "location.remove",
    "keymap.unset",
    "mousemap.unset",
    "midimap.unset",
];

#[test]
fn the_command_table_says_the_same_words_as_the_rows() {
    let (s, _) = studio();
    for d in s.song.registry().all() {
        let Some(label) = d.label() else { continue };
        for (old, new) in OLD_WORDS {
            if *old == "外す" && TAKE_OFF.contains(&d.name) {
                continue;
            }
            // The preset is chosen from a list, as a number is.
            if *old == "選ぶ" && d.name == "plugin.preset.choose" {
                continue;
            }
            // "取り消す" is to undo, not to delete: the last lap is taken back, as the mock says.
            if *old == "消す" && d.name == "take.undo-lap" {
                continue;
            }
            assert!(
                !label.contains(old),
                "{}: 「{label}」の「{old}」は「{new}」",
                d.name
            );
        }
    }
}

/// One operation, one name: the rows that do the same thing in different scenes, and the scenes
/// that must say it.
const SAME: &[(&str, &[&str])] = &[
    ("ここにポイントを追加", &["auto.rs", "tempo.rs", "roll.rs"]),
    ("次のポイントまでの形", &["auto.rs", "tempo.rs", "roll.rs"]),
    (
        "すべてのポイントを削除",
        &["auto.rs", "mixer.rs", "roll.rs"],
    ),
    ("このレーンを隠す", &["auto.rs", "roll.rs"]),
    ("レーンを追加", &["auto.rs"]),
    ("値を入力…", &["auto.rs", "mixer.rs", "roll.rs"]),
    ("プラグインを削除", &["mixer.rs", "../instrument/menu.rs"]),
    ("再起動", &["mixer.rs", "../instrument/menu.rs"]),
    (
        "オートメーションを表示",
        &["mixer.rs", "track.rs", "../instrument/menu.rs"],
    ),
    ("センドを削除", &["mixer.rs"]),
    ("トラックを追加", &["mixer.rs", "track.rs"]),
    ("ステムを書き出す…", &["track.rs"]),
    ("エフェクトを追加…", &["mixer.rs"]),
    ("このバスを書き出す…", &["mixer.rs"]),
    ("同じ高さのノートを選択", &["roll.rs"]),
    ("中のトラックを選択", &["track.rs"]),
];

#[test]
fn one_operation_has_one_name_in_every_scene() {
    let all = all_sources();
    for (name, files) in SAME {
        for file in *files {
            let (_, src) = all
                .iter()
                .find(|(f, _)| f == file)
                .unwrap_or_else(|| panic!("no source {file}"));
            assert!(
                literals(src).iter().any(|(_, s)| s == name),
                "{file}: 「{name}」の行が無い"
            );
        }
    }
}

/// The rows of the menu now open: short, and no name twice.
fn check_open_menu(scene: &str, s: &crate::studio::Studio) {
    let rows = labels(s);
    assert!(!rows.is_empty(), "{scene}: no menu");
    for (i, a) in rows.iter().enumerate() {
        assert!(a.chars().count() <= 20, "{scene}: 「{a}」が長い");
        assert!(!a.contains("..."), "{scene}: 「{a}」の ... は …");
        assert!(!a.ends_with('。'), "{scene}: 「{a}」");
        assert!(!rows[..i].contains(a), "{scene}: 「{a}」が 2 回");
    }
}

#[test]
fn the_rows_of_the_arrangement_scenes_are_short_and_named_once() {
    let (mut s, g) = studio();
    let (x, y) = lane_at(&s, &g, "Keys", 13);
    assert!(s.scene_context(&g, x, y));
    check_open_menu("empty lane", &s);

    let (x, y) = clip_mid(&s, &g, "Keys", 0);
    assert!(s.scene_context(&g, x, y));
    check_open_menu("clip", &s);

    let bar = s.tick_x(&g, 2 * BAR) + 2.0;
    assert!(s.scene_context(&g, bar, g.ruler.y + g.ruler.h - 4.0));
    check_open_menu("ruler", &s);
    assert!(s.scene_context(&g, bar, g.ruler.y + 4.0));
    check_open_menu("loop band", &s);

    // The track's menu is tall enough to cover the folder's row, so the row is found first.
    let f = folder(&s, "Rhythm");
    let (fx, fy) = row_mid(&s, &g, RowKind::Folder(f));
    let t = track(&s, "Vocal");
    let (x, y) = row_mid(&s, &g, RowKind::Track(t));
    assert!(s.scene_context(&g, x, y));
    check_open_menu("track", &s);

    assert!(s.scene_context(&g, fx, fy));
    check_open_menu("folder", &s);

    s.song
        .run("marker.add", serde_json::json!({"at": 4 * BAR}))
        .unwrap();
    let id = s.project().markers[0].id;
    let r = s.marker_rect(&g, id).unwrap();
    assert!(s.scene_context(&g, r.x + 4.0, r.y + 4.0));
    check_open_menu("marker", &s);
}
