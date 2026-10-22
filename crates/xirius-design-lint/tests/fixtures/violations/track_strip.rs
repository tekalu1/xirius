// トラックの色を細い矩形に塗る（レーン・頭の端の色の帯）。トラックの色は線と点だけ。
fn lanes(pen: &mut Pen, track: &Track, g: &Geometry, top: f32) {
    let color = track_ink(pen, track);
    let color = if track.auto_mode.plays() { color } else { alpha(color, 0.5) };
    let faded = alpha(color, 0.5);
    pen.fill(Rect::new(g.lanes.x, top, 3.0, LANE_H), color); //~ one-sided-edge
    let edge = Rect::new(g.column.x, top, 2.0, LANE_H);
    pen.round(at(edge, top), faded, Radius::Clip); //~ one-sided-edge
    pen.fill(Rect::new(0.0, top, g.lanes.w, 4.0), pen.c.tracks[1]); //~ one-sided-edge
}
