// 構造線（フォルダーの範囲の線）。太さと位置を StructureLine から取った物だけが通る。
use xirius_design::{LineState, Radius, StructureLine, Theme};

fn folder_range_line(theme: Theme, depth: usize, top: f32, height: f32) {
    let line = div()
        .absolute()
        .left(px(StructureLine::gutter_x(depth)))
        .top(px(top + StructureLine::END_INSET))
        .w(px(StructureLine::THICKNESS))
        .h(px(height - StructureLine::END_INSET * 2.0))
        .rounded(px(StructureLine::CAP.px()))
        .bg(StructureLine::color(LineState::Rest, theme));
}
