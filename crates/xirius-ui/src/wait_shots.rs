//! `--script wait`: how waiting looks (UI-061) on a real window. Each stage reports what it
//! expects on stderr (`wait-check stage=N ok=true`), which `tests/wait_window.rs` reads. The
//! veil's Esc is a real key event on the window.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{App, KeyDownEvent, Keystroke, Modifiers, PlatformInput, Window};

use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;
use crate::wait::{self, Id, Kind, OVERLAY_AFTER, REFLECTING_AFTER};

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const WAIT: &[Shot] = &[
    Shot {
        name: "wait-01-line",
        acts: &[staged(0)],
    },
    // A job that ends inside 0.2 s: nothing shows.
    Shot {
        name: "wait-02-quick-no-veil",
        acts: &[staged(1)],
    },
    Shot {
        name: "wait-03-veil",
        acts: &[staged(2)],
    },
    Shot {
        name: "wait-04-esc-cancelled",
        acts: &[staged(3)],
    },
    Shot {
        name: "wait-05-reflecting",
        acts: &[staged(4)],
    },
    Shot {
        name: "wait-06-done",
        acts: &[staged(5)],
    },
];

thread_local! {
    static LINE: Cell<Option<Id>> = const { Cell::new(None) };
    static JOB: Cell<Option<Id>> = const { Cell::new(None) };
    static START: Instant = Instant::now();
}

fn escape(window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: Keystroke {
                modifiers: Modifiers::default(),
                key: "escape".into(),
                key_char: None,
            },
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
}

pub(crate) fn play(stage: u8, _studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let at = START.with(|start| *start + Duration::from_secs(u64::from(stage)));
    wait::script_time(Some(at));
    let ok = match stage {
        0 => {
            LINE.set(Some(wait::begin(Kind::Line, "波形")));
            let v = wait::view();
            v.line && v.veil.is_none()
        }
        1 => {
            let id = wait::begin(Kind::Block, "取り込み");
            wait::script_time(Some(at + OVERLAY_AFTER - Duration::from_millis(1)));
            let quiet = wait::view().veil.is_none();
            wait::end(id);
            quiet && wait::view().veil.is_none()
        }
        2 => {
            JOB.set(Some(wait::begin(Kind::Block, "取り込み")));
            let early = wait::view().veil.is_none();
            // Esc before the veil is up is not taken by the job.
            let kept = !wait::cancel();
            wait::script_time(Some(at + OVERLAY_AFTER));
            early && kept && wait::view().veil.as_deref() == Some("取り込み")
        }
        3 => {
            let id = JOB.take().expect("the job of stage 2");
            escape(window, cx);
            let ok = wait::cancelled(id) && wait::view().veil.is_none();
            wait::end(id);
            ok
        }
        4 => {
            let id = wait::begin(Kind::Reflect, "反映");
            JOB.set(Some(id));
            let v = wait::view();
            let early = v.hatch && !v.reflecting;
            wait::script_time(Some(at + REFLECTING_AFTER));
            early && wait::view().reflecting
        }
        _ => {
            if let Some(id) = JOB.take() {
                wait::end(id);
            }
            if let Some(id) = LINE.take() {
                wait::end(id);
            }
            wait::view() == wait::View::default()
        }
    };
    eprintln!("wait-check stage={stage} ok={ok}");
    if stage == 5 {
        wait::script_time(None);
    }
}
