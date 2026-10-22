// 生の角丸。
fn radius() {
    let a = div().rounded(8.0); //~ raw-radius
    let b = div().rounded_md(); //~ raw-radius
    let c = div().rounded_lg(); //~ raw-radius
    let d = div().corner_radius(6.0); //~ raw-radius
    let e = div().border_radius(4); //~ raw-radius
    let f = div().rounded(px(8.0)); //~ raw-radius
    let ok_none = div().rounded_none();
    let ok_token = div().rounded(Radius::M.px());
}
