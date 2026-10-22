// 画面の文字の中の、操作を教える文（`ui_` で始まる見本は `xirius-ui` の画面のファイルとして検査する）。
fn guidance(name: &str) {
    let a = "点がありません。クリックで追加します"; //~ guidance
    let b = "点がありません。鉛筆に持ち替えて追加します"; //~ guidance
    let c = format!("「{name}」を隠しました。検索すると出ます"); //~ guidance
    let d = "先にトラックを録音準備にしてください"; //~ guidance
    let e = "開いた曲がここに並びます"; //~ guidance
    // 状態と結果だけの文は通る。
    let f = "点がありません";
    let g = "「Dexed」を隠しました";
    let h = "録音準備のトラックがありません";
    // コメントの「してください」は見ない。
    let i = 1;
}

#[cfg(test)]
mod tests {
    #[test]
    fn guidance_in_tests_is_fine() {
        assert!(!"クリックで追加します".is_empty());
    }
}
