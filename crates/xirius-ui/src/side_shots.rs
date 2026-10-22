//! `--script side`: the right edges of the browser's panel and of the track head
//! (UI-048), dragged as real pointer events on the window. Each stage reports the
//! width it expects on stderr (`side-check stage=N ok=true`), which
//! `tests/side_window.rs` reads. The edge's cursor does not show in a window
//! picture; the tests in `side_edge` hold it.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    PlatformInput, Point, Window, point, px,
};
use xirius_design::Layout;

use crate::shot::{Act, Play, Shot};
use crate::side_edge::{BROWSER_MIN_W, Side};
use crate::studio::{Geometry, Rect, Studio};

/// Stage `n` of this script, played by [`play`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SIDE: &[Shot] = &[
    Shot {
        name: "side-01-browser-open",
        acts: &[staged(0)],
    },
    // The panel finishes opening (it slides) before an edge is taken.
    Shot {
        name: "side-01b-browser-settled",
        acts: &[],
    },
    Shot {
        name: "side-02-browser-wider",
        acts: &[staged(1)],
    },
    Shot {
        name: "side-03-head-wider",
        acts: &[staged(2)],
    },
    Shot {
        name: "side-04-browser-at-least",
        acts: &[staged(3)],
    },
    Shot {
        name: "side-05-browser-double-click",
        acts: &[staged(4)],
    },
    // It slides back to the width it opens with.
    Shot {
        name: "side-06-browser-back",
        acts: &[staged(5)],
    },
];

fn at(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y + Layout::TOP_BAR_H))
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn button(down: bool, p: Point<Pixels>, clicks: usize) -> PlatformInput {
    if down {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: p,
            modifiers: Modifiers::default(),
            click_count: clicks,
            first_mouse: false,
        })
    } else {
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: p,
            modifiers: Modifiers::default(),
            click_count: clicks,
        })
    }
}

fn moved(p: Point<Pixels>, held: bool) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position: p,
        pressed_button: held.then_some(MouseButton::Left),
        modifiers: Modifiers::default(),
    })
}

fn geometry(studio: &Rc<RefCell<Studio>>, window: &Window) -> Geometry {
    crate::screen::geometry(&studio.borrow(), window)
}

/// The middle of `side`'s edge, as the window sees it now.
fn edge(studio: &Rc<RefCell<Studio>>, window: &Window, side: Side) -> Option<(f32, f32)> {
    let g = geometry(studio, window);
    studio.borrow().side_edge(&g, side).map(mid)
}

/// Press on the edge, move by `dx` in four steps with the button held (the pointer
/// stays inside the window), let go.
fn drag(side: Side, dx: f32, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let Some(from) = edge(studio, window, side) else {
        return eprintln!("side-check no edge for {side:?}");
    };
    let start = at(from.0, from.1);
    window.dispatch_event(moved(start, false), cx);
    window.dispatch_event(button(true, start, 1), cx);
    let mut last = start;
    for i in 1..=4 {
        let x = (from.0 + dx * i as f32 / 4.0).max(2.0);
        last = at(x, from.1);
        window.dispatch_event(moved(last, true), cx);
    }
    window.dispatch_event(button(false, last, 1), cx);
}

fn double_click(side: Side, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let Some(from) = edge(studio, window, side) else {
        return eprintln!("side-check no edge for {side:?}");
    };
    let p = at(from.0, from.1);
    window.dispatch_event(moved(p, false), cx);
    for n in 1..=2 {
        window.dispatch_event(button(true, p, n), cx);
        window.dispatch_event(button(false, p, n), cx);
    }
}

/// What the stages compare with: the widths before the one that is played.
#[derive(Clone, Copy)]
struct Widths {
    browser: f32,
    head: f32,
}

thread_local! {
    static WAS: std::cell::Cell<Option<Widths>> = const { std::cell::Cell::new(None) };
}

fn widths(studio: &Rc<RefCell<Studio>>, window: &Window) -> Widths {
    let g = geometry(studio, window);
    Widths {
        browser: g.panel.w,
        head: g.column.w,
    }
}

pub(crate) fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let before = WAS.with(|w| w.get());
    match stage {
        0 => studio.borrow_mut().browser.open = true,
        1 => drag(Side::Browser, 100.0, studio, window, cx),
        2 => drag(Side::Head, 60.0, studio, window, cx),
        3 => drag(Side::Browser, -2000.0, studio, window, cx),
        4 => double_click(Side::Browser, studio, window, cx),
        _ => {}
    }
    let now = widths(studio, window);
    // Widths come out of float layout arithmetic; a hundredth of a pixel is the same width.
    let same = |a: f32, b: f32| (a - b).abs() < 0.01;
    let ok = match (stage, before) {
        (0, _) => same(now.browser, Layout::BROWSER_W),
        (1, Some(b)) => same(now.browser, b.browser + 100.0) && same(now.head, b.head),
        (2, Some(b)) => same(now.head, b.head + 60.0) && same(now.browser, b.browser),
        (3, _) => same(now.browser, BROWSER_MIN_W),
        (4, _) => studio.borrow().side_size.browser.is_none(),
        (5, _) => same(now.browser, Layout::BROWSER_W),
        _ => false,
    };
    eprintln!(
        "side-check stage={stage} browser={} head={} ok={ok}",
        now.browser, now.head
    );
    WAS.with(|w| w.set(Some(now)));
}
