//! UI-309: 有彩色の塗りの面の上は ink_on_fill、mute・solo の札はその対の色。

fn toggle_button(pen: &mut Pen, c: &Colors, r: Rect, on: bool, hover: bool) {
    let (face, ink) = pen.transition_pair(
        motion,
        setting,
        r,
        key,
        on,
        toggle_colors(c, false, hover),
        toggle_colors(c, true, hover),
    );
    pen.round(r, face, Radius::S);
    pen.icon(icon, r, ink);
}

fn play(pen: &mut Pen, c: &Colors, r: Rect, hovered: bool) {
    let (face, ink) = if hovered { c.primary_hover } else { (c.surface_3, c.ink) };
    pen.round(r, face, Radius::S);
    pen.icon(Icon::Play, icon_at(r), ink);
}

fn chip(pen: &mut Pen, c: &Colors, r: Rect, on: bool) {
    let (face, ink) = if on { (c.fill, c.on_fill) } else { (c.surface_3, c.ink) };
    pen.round(r, face, Radius::S);
    pen.text("Chip", size, weight, ink, r, align);
}

// mute・solo の札は、その対の色。
fn mute(pen: &mut Pen, c: &Colors, r: Rect) {
    let (face, ink) = c.mute_on;
    pen.round(r, face, Radius::S);
    pen.text("M", size, weight, ink, r, align);
}

// 塗りの輪の内側に地の面を置けば、その上の文字は地の色。
fn field(pen: &mut Pen, c: &Colors, r: Rect) {
    pen.round(r, c.fill, Radius::S);
    pen.round(Rect::new(r.x + 1.5, r.y + 1.5, r.w - 3.0, r.h - 3.0), c.surface_3, Radius::S);
    pen.digits("4", size, weight, c.ink_strong, r, align);
}

// 塗りの面の横の文字は面の外。
fn beside(pen: &mut Pen, c: &Colors, r: Rect) {
    pen.round(r, c.fill, Radius::S);
    pen.text("Label", size, weight, c.ink, Rect::new(r.right() + 4.0, r.y, 40.0, r.h), align);
}
