// 押せる物が形を持つ・右の押下・離すだけ・親が形を決めている（理由付き）。何も出ない。
fn button(cx: &mut Context<View>) -> Div {
    div()
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, _| {}))
}

fn link() -> Stateful<Div> {
    div().id("link").cursor_pointer().on_click(|_, _, _| {})
}

fn menu_and_release(cx: &mut Context<View>) -> Div {
    div()
        .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, _| {}))
        .on_mouse_up(MouseButton::Left, cx.listener(|_, _, _, _| {}))
}

fn inner(cx: &mut Context<View>) -> Div {
    div()
        // design-lint: allow(cursor) 形は外側の要素が当たりから決める
        .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, _| {}))
}

#[cfg(test)]
mod tests {
    fn probe() -> Div {
        div().on_click(|_, _, _| {})
    }
}
