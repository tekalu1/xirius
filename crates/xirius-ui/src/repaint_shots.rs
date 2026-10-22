//! The measurement of UI-039 on a real window (`xirius-ui --park --silent --script repaint
//! --shot <dir>`): while nothing the overview shows changes, the screen is rendered every
//! frame and the cached overview is not painted again; when something it shows changes (the
//! pointer moves onto a button), it is painted once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, Window};

use crate::roll::layer::counts;
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::Studio;

/// Stage `n` of this script, played by [`measure`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(measure), n)
}

pub const REPAINT: &[Shot] = &[
    // The screen starts asking for every frame, as it does while something moves.
    Shot {
        name: "repaint-00-start",
        acts: &[staged(0)],
    },
    // Nothing changes from here: the count begins and the next picture comes after more frames.
    Shot {
        name: "repaint-01-still",
        acts: &[staged(1)],
    },
    // The pointer moves onto a button the overview shows as hot.
    Shot {
        name: "repaint-02-hover",
        acts: &[staged(2), Act::Hover(Target::Mute("Drums"))],
    },
    Shot {
        name: "repaint-03-hover-after",
        acts: &[staged(3)],
    },
];

/// The run asks for each frame the way the screen does while something moves, and not as a
/// refresh of the whole window (which paints every cached view again).
static STILL: AtomicBool = AtomicBool::new(false);

/// Whether the frames now are the ones being measured.
pub fn still_frames() -> bool {
    STILL.load(Ordering::Relaxed)
}

thread_local! {
    /// `(screen renders, overview paints)` when the measure began.
    static SINCE: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
}

fn measure(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, _: &mut App) {
    let now = counts();
    STILL.store(n < 3, Ordering::Relaxed);
    match n {
        // One refresh gets the frames going; the overview is painted once by it.
        0 => window.refresh(),
        1 => SINCE.with(|s| s.set(now)),
        _ => {
            let (renders, paints) = SINCE.with(Cell::get);
            let (renders, paints) = (now.0 - renders, now.1 - paints);
            let ok = studio.borrow().error.is_none()
                && renders >= MIN_FRAMES
                && if n == 2 {
                    paints == 0
                } else {
                    (1..=MAX_PAINTS_FOR_A_CHANGE).contains(&paints)
                };
            eprintln!("repaint-check stage={n} renders={renders} overview_paints={paints} ok={ok}");
            SINCE.with(|s| s.set(now));
        }
    }
}

/// Frames the driver lets pass between two of these stages (its settle frames and more).
const MIN_FRAMES: u64 = 8;
/// One change repaints the overview once (the hot button is lit); a second paint would be
/// another change the run did not make.
const MAX_PAINTS_FOR_A_CHANGE: u64 = 2;
