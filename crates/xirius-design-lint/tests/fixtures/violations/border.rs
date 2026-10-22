// 線。
fn borders() {
    let a = div().border_1(); //~ border
    let b = div()
        .p_2()
        .border_color(x) //~ border
        .border_b_1(); //~ border
    let c = div().border(px(1.0)); //~ border
    let d = div().border_dashed(); //~ border
    let not_a_border = div().bordered_thing();
}
