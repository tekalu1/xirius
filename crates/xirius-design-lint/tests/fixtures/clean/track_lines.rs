// トラックの色は線と点に使う。面は中立の色。細い矩形でも中立の色なら通す。
fn lanes(pen: &mut Pen, track: &Track, g: &Geometry, top: f32) {
    let color = track_ink(pen, track);
    polyline(pen, &line, color);
    dot(pen, x, y, POINT, color, pen.c.surface_1);
    pen.fill(Rect::new(g.lanes.x, top, g.lanes.w, LANE_H), pen.c.surface_1);
    pen.round(Rect::new(x, y, Layout::DEPTH_DOT, Layout::DEPTH_DOT), pen.c.ink, Radius::Pill);
    // 広い面にトラックの色の中身（クリップの中の音符）を置くのは帯ではない。
    pen.fill(Rect::new(x, y, w, h), color);
    // 小さな点。
    pen.round(Rect::new(x - 2.0, y - 2.0, 4.0, 4.0), color, Radius::Xs);
    for tick in ticks {
        // 同じ名前を中立の色に使い直した、格子の線。
        let color = pen.c.surface_2;
        pen.fill(Rect::new(x, y, 1.0, h), color);
    }
}
