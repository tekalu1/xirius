#[test]
fn terminology_table_covers_daw_terms() {
    let doc = include_str!("../../../docs/design-rules.md");
    for (before, after) in [
        ("格子", "グリッド"),
        ("吸着", "スナップ"),
        ("寄せる", "クオンタイズ"),
        ("素通し", "バイパス"),
        ("強さ", "ベロシティ"),
        ("版", "バージョン"),
        ("録音待ち", "録音準備"),
    ] {
        assert!(
            doc.lines().any(|line| line.starts_with("| ")
                && line.contains(before)
                && line.contains(after)),
            "{before} / {after}"
        );
    }
}
