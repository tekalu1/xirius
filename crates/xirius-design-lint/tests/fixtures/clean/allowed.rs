// 理由を付けた例外。何も出ない。
fn allowed() {
    // design-lint: allow(raw-color) 外部の仕様が決めた色（ここだけ）
    let a = rgb(0x336699);
    let b = rgb(0x336699); // design-lint: allow(raw-color) 同じ行に置く例外
    // design-lint: allow(border) 連なりの先頭の行に置いても、連なり全体に効く
    let c = div()
        .p_2()
        .border_1();
    // design-lint: allow(raw-color, raw-radius) 規則を並べて外す
    let d = div().rounded(8.0).bg(rgb(0x000000));
}
