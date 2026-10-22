//! The crash records entry, reached with real window input after a previous process failed.
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use gpui::{App, Window};
use std::{cell::RefCell, rc::Rc};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "crash01-about-entry",
        acts: &[Act::Reset, Act::Press(Target::Top(BarButton::Logo))],
    },
    Shot {
        name: "crash02-records",
        acts: &[
            Act::Press(Target::PopupText("クラッシュ記録")),
            Act::Stage(Play::Window(check), 0),
        ],
    },
];

fn check(_: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, _: &mut App) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let popup = s.popup.as_ref().unwrap();
    let records = popup.about.as_ref().unwrap().crashes.as_ref().unwrap();
    assert!(records.error.is_none());
    let cells = popup.layout(g.size).cells;
    for (i, record) in records.records.iter().enumerate().take(4) {
        assert!(record.log.is_file() && record.dump.metadata().unwrap().len() > 0);
        assert!(cells.iter().any(|c| c.action
            == Some(crate::popup::Action::About(crate::about::Act::Crash(
                crate::crash_reports::Act::Log(i)
            )))));
    }
    eprintln!("crash-entry records={}", records.records.len());
}
