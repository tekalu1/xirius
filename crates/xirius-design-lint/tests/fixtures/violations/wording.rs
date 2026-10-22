// 文言の硬い言い回し。
fn wording(n: usize) {
    let a = "プラグインのスキャンが要ります"; //~ wording
    let b = format!("--{n} が要る"); //~ wording
    let c = Cmd::new("snap", "格子に吸着する"); //~ wording
    let d = "版の一覧"; //~ wording
    let e = "スナップをオンにしました";
    let f = "クオンタイズの強さ";
    // コメントの「要ります」は見ない。
    let g = 1;
}

#[cfg(test)]
mod tests {
    #[test]
    fn old_words_in_tests_are_fine() {
        assert!(!"格子".is_empty());
    }
}
