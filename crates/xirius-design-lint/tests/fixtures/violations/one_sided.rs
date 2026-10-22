// 角丸の物の 1 辺だけの色。中立の色の光（面の上辺の光）も、端に貼り付いた帯も同じ。
fn one_sided() {
    let a = div()
        .rounded(Radius::M.px())
        .border_l_2(); //~ border, one-sided-edge
    let b = div()
        .rounded(Radius::S.px())
        .border_t_1() //~ border, one-sided-edge
        .border_b_1(); //~ border, one-sided-edge
    let strip = div()
        .absolute()
        .left_0()
        .w(px(3.0))
        .bg(Ink::Blue.color(theme)); //~ one-sided-edge, color-family
    let bottom_strip = div()
        .absolute()
        .bottom_0()
        .h(px(2.0))
        .background(Ink::Alert.color(theme)); //~ one-sided-edge
    let top_light = div()
        .absolute()
        .top_0()
        .h(px(1.0))
        .bg(Surface::S3.color(theme)); //~ one-sided-edge
    let upper_shadow = BoxShadow { //~ one-sided-edge
        color: c,
        offset: point(px(0.0), px(-1.0)),
        blur_radius: px(0.0),
        spread_radius: px(0.0),
    };
}
