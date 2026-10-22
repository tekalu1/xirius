//! UI-309: 有彩色の塗りの面の上の文字・アイコン・印が、その面の色（ink_on_fill）でない。

// 入ったトグル（青い面）のアイコンを、地の文字の色で描いた（もとの不具合）。
fn toggle_button(pen: &mut Pen, c: &Colors, r: Rect, on: bool, hover: bool) {
    let off = if hover { c.hover } else { c.surface_1 };
    let face = pen.transition(motion, setting, r, key, on, off, c.fill);
    pen.round(r, face, Radius::S);
    pen.icon(icon, r, c.ink); //~ fill-ink
}

// 止まっている再生ボタンに乗ると、面だけが fill_hover になる。
fn play(pen: &mut Pen, c: &Colors, r: Rect, hovered: bool) {
    let face = if hovered { c.fill_hover } else { c.surface_3 };
    pen.round(r, face, Radius::S);
    pen.icon(Icon::Play, icon_at(r), c.ink); //~ fill-ink
}

// 面と文字を組で選んでも、文字が面の対でなければ落ちる。
fn chip(pen: &mut Pen, c: &Colors, r: Rect, on: bool) {
    let (face, ink) = if on { (c.fill, c.ink_strong) } else { (c.surface_3, c.ink) };
    pen.round(r, face, Radius::S);
    pen.text("Chip", size, weight, ink, r, align); //~ fill-ink
}

// フェーダーのつまみの溝（印）。
fn cap(pen: &mut Pen, c: &Colors, cap: Rect, hot: bool) {
    let face = if hot { c.fill_hover } else { c.thumb };
    pen.round(cap, face, Radius::S);
    pen.fill(Rect::new(cap.x + 2.0, mid, cap.w - 4.0, 1.0), c.ink_strong); //~ fill-ink
}

// 危険の面の上の数字。
fn danger(pen: &mut Pen, c: &Colors, r: Rect) {
    pen.round(r, c.danger, Radius::S);
    pen.digits("3", size, weight, c.ink_blue, r, align); //~ fill-ink
}

// 塗りの面の上の SVG の絵（Pen::svg）も、アイコンと同じに見る。
fn mark(pen: &mut Pen, c: &Colors, r: Rect) {
    pen.round(r, c.fill, Radius::S);
    pen.svg(path, r, c.ink); //~ fill-ink
}
