// 時間のグリッドの線（UI-339）。幅と位置を GridLine から取った線だけが、グリッドの色で塗れる。
use xirius_design::{GridLine, GridTier};

fn lines(pen: &mut Pen, c: &Colors, xs: &[f32], top: f32, h: f32) {
    for &x in xs {
        let tier = GridTier::Beat;
        pen.fill(
            Rect::new(GridLine::left(x, tier), top, GridLine::width(tier), h),
            c.grid[tier.index()],
        );
        let r = Rect::new(GridLine::left(x, GridTier::Bar), top, GridLine::width(GridTier::Bar), h);
        pen.fill(r, c.grid[0].opacity(0.5));
    }
}

fn column(theme: Theme, x: f32) {
    let tier = GridTier::Bar;
    let line = div()
        .absolute()
        .left(px(GridLine::left(x, tier)))
        .w(px(GridLine::width(tier)))
        .h_full()
        .bg(GridLine::color(tier, theme));
}
