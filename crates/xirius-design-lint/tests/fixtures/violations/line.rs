// 細い線。固定値の太さの面は、区切りの線として止める（例外は StructureLine だけ）。
fn lines() {
    let divider = div()
        .w_full()
        .h(px(1.0))
        .bg(Surface::S3.color(theme)); //~ line
    let vertical = div()
        .h_full()
        .w(px(2.0))
        .rounded(px(Radius::Xs.px()))
        .bg(Ink::Blue.color(theme)); //~ line, color-family
}
