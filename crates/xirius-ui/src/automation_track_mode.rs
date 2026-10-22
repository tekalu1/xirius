//! The track head's mode icon (AUT-006), cycling as in the approved mock.
use crate::{
    arrange::{LaneButton, Spot},
    draw::Pen,
    studio::{Rect, Studio},
    tree::Row,
};
use xirius_core::{
    auto::{AutoMode, AutoTarget},
    id::TrackId,
};
use xirius_design::{Layout, Radius, Space};

pub fn rect(auto: Rect) -> Rect {
    Rect::new(
        auto.x - Space::X1.px() - Layout::ICON_SIZE,
        auto.y,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    )
}
pub fn next(mode: AutoMode) -> AutoMode {
    match mode {
        AutoMode::Off | AutoMode::Write => AutoMode::Read,
        AutoMode::Read => AutoMode::Touch,
        AutoMode::Touch => AutoMode::Latch,
        AutoMode::Latch => AutoMode::Write,
    }
}
pub fn spot(track: TrackId, mode: AutoMode) -> Spot {
    Spot::LaneHead {
        track,
        target: AutoTarget::Volume,
        button: LaneButton::Mode(mode),
    }
}
pub fn paint(
    pen: &mut Pen,
    studio: &Studio,
    row: &Row,
    track: TrackId,
    mode: AutoMode,
    left: f32,
    top: f32,
) {
    let Some(auto) = crate::studio::RowParts::of(row).auto else {
        return;
    };
    let r = rect(auto);
    let r = Rect::new(left + r.x, top + r.y, r.w, r.h);
    let hot = studio.arrange.hover == spot(track, mode);
    if hot || mode != AutoMode::Off {
        pen.round(
            r,
            if hot { pen.c.hover } else { pen.c.surface_3 },
            Radius::Clip,
        );
    }
    let ink = match mode {
        AutoMode::Latch | AutoMode::Write => pen.c.alert,
        AutoMode::Off => pen.c.ink,
        _ => pen.c.ink_strong,
    };
    pen.icon(crate::arrange::paint::mode_icon(mode), r, ink);
}
