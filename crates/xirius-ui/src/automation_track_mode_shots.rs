//! Cycle the mode at a track head while its lanes are shut.
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::{Geometry, RowParts, Studio},
    tree::RowKind,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, Window, point, px,
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use xirius_core::{auto::AutoMode, id::TrackId};
use xirius_design::Layout;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "head01-read",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(setup), 0),
            Act::Press(Target::Track("Keys")),
        ],
    },
    Shot {
        name: "head02-touch",
        acts: &[Act::Stage(Play::Window(press), 1)],
    },
    Shot {
        name: "head03-latch",
        acts: &[Act::Stage(Play::Window(press), 2)],
    },
    Shot {
        name: "head04-write",
        acts: &[Act::Stage(Play::Window(press), 3)],
    },
    Shot {
        name: "head05-read",
        acts: &[Act::Stage(Play::Window(press), 4)],
    },
    Shot {
        name: "head06-off",
        acts: &[Act::Key("j"), Act::Stage(Play::Studio(verify), 5)],
    },
    Shot {
        name: "head07-read",
        acts: &[Act::Key("j"), Act::Stage(Play::Studio(verify), 6)],
    },
    Shot {
        name: "head08-palette-off",
        acts: &[
            Act::Key("ctrl-k"),
            Act::Key("a"),
            Act::Key("u"),
            Act::Key("t"),
            Act::Key("o"),
            Act::Key("."),
            Act::Key("m"),
            Act::Key("o"),
            Act::Key("d"),
            Act::Key("e"),
            Act::Key("enter"),
            Act::Stage(Play::Studio(verify), 5),
        ],
    },
    Shot {
        name: "head09-palette-read",
        acts: &[
            Act::Key("ctrl-k"),
            Act::Key("a"),
            Act::Key("u"),
            Act::Key("t"),
            Act::Key("o"),
            Act::Key("."),
            Act::Key("m"),
            Act::Key("o"),
            Act::Key("d"),
            Act::Key("e"),
            Act::Key("enter"),
            Act::Stage(Play::Studio(verify), 6),
        ],
    },
];
fn track(s: &Studio) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id
}
fn setup(s: &mut Studio, _: u8) {
    s.drawer_open = false;
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: false };
    s.run_command("auto.mode", json!({"ids":[track(s)],"mode":"read"}));
    s.scroll.1 = s.tree().row_of(RowKind::Track(track(s))).unwrap().y;
    assert!(!s.arrange.open.contains(&track(s)));
}
fn position(s: &Studio, g: &Geometry) -> (f32, f32) {
    let tree = s.tree();
    let row = tree.row_of(RowKind::Track(track(s))).unwrap();
    let r = crate::automation_track_mode::rect(RowParts::of(row).auto.unwrap());
    (
        g.column.x + r.x + r.w / 2.0,
        s.row_y(g, row) + r.y + r.h / 2.0,
    )
}
fn check(s: &Studio, n: u8) {
    let expected = [
        AutoMode::Read,
        AutoMode::Touch,
        AutoMode::Latch,
        AutoMode::Write,
        AutoMode::Read,
        AutoMode::Off,
        AutoMode::Read,
    ][n as usize];
    assert_eq!(s.project().tracks[&track(s)].auto_mode, expected);
    assert!(!s.arrange.open.contains(&track(s)));
    assert!(s.error.is_none(), "{:?}", s.error);
}
fn verify(s: &mut Studio, n: u8) {
    check(s, n);
}
fn press(n: u8, s: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let (x, y) = {
        let s = s.borrow();
        position(&s, &crate::screen::geometry(&s, window))
    };
    let position = point(px(x), px(y + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
    check(&s.borrow(), n);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_closed_track_head_cycles_four_modes_in_one_step_and_read_can_still_be_turned_off() {
        let mut s = Studio::sample();
        setup(&mut s, 0);
        let g = s.geometry(1680.0, 1000.0);
        let t = track(&s);
        s.selected_row = Some(RowKind::Track(t));
        let before = s.project();
        for n in 1..=4 {
            let (x, y) = position(&s, &g);
            assert_eq!(
                s.arrange_spot(&g, x, y),
                crate::automation_track_mode::spot(t, s.project().tracks[&t].auto_mode)
            );
            assert!(s.arrange_down(&g, x, y, crate::tools::Mods::default()));
            s.arrange_up(&g, x, y, crate::tools::Mods::default());
            check(&s, n);
        }
        s.open_palette(&g);
        let row = s
            .palette_rows(&g)
            .into_iter()
            .find(|r| r.name == "auto.mode")
            .unwrap();
        assert!(row.enabled);
        assert!(s.run_row(&g, &row));
        assert_eq!(s.project().tracks[&t].auto_mode, AutoMode::Off);
        assert!(s.song.undo());
        assert_eq!(s.project(), before);
        s.selected_row = None;
        assert!(
            !s.palette_rows(&g)
                .iter()
                .find(|r| r.name == "auto.mode")
                .unwrap()
                .enabled
        );
    }
}
