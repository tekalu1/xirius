// 時間のグリッドの色を、GridLine を通さずに塗る。
fn lines(pen: &mut Pen, c: &Colors, x: f32, top: f32, h: f32, r: Rect) {
    pen.fill(Rect::new(x, top, 1.0, h), c.grid[1]); //~ line
    pen.round(r, c.grid[0].opacity(0.5), radius); //~ line
    div().bg(c.grid[2]); //~ color-family
}

// GridLine の字はあるが、幅を GridLine から取っていない（線でなく面になる）。
fn wide(pen: &mut Pen, c: &Colors, theme: Theme, x: f32, top: f32, h: f32) {
    let tier = GridTier::Bar;
    pen.fill(Rect::new(GridLine::left(x, tier), top, 40.0, h), c.grid[0]); //~ line
    pen.fill(Rect::new(x, top, 40.0, h + GridLine::width(tier)), c.grid[0]); //~ line
    let r = Rect::new(GridLine::left(x, tier), top, 1.0, h);
    pen.fill(r, c.grid[0]); //~ line
    let band = div()
        .absolute()
        .left(px(GridLine::left(x, tier)))
        .w_full()
        .bg(GridLine::color(tier, theme)); //~ color-family
}
