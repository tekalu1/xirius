// 左の押下・クリック・ドラッグを受ける要素に形が無いと落ちる（UI-385）。
fn button(cx: &mut Context<View>) -> Div {
    div()
        .size_full()
        .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, _| {})) //~ cursor
}

fn row() -> Stateful<Div> {
    div().id("row").on_click(|_, _, _| {}) //~ cursor
}

fn outer() -> Div {
    div().child(
        div().on_drag((), |_, _, _, cx| cx.new(|_| Empty)), //~ cursor
    )
}
