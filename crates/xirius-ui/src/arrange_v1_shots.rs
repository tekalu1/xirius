//! `--script arrange-v1`: the arranging rows of the approved 1.0 form (ux-v1-arrange 01 to 04),
//! played as real pointer and key events on the window. A section dragged to a new place (the
//! glow on the moving span, the drop line) and copied with Alt; the right-click menus of a
//! section, of the empty band and of the loop; a section painted with the pencil; time put in
//! through the keys; and the track list folded to its strip.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PlatformInput, Window, point, px,
};
use serde_json::json;
use xirius_core::time::Tick;
use xirius_design::Layout;

use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;
use crate::tools::Tool;

/// Stage `n` of [`SHOTS`], played through the window.
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    // The sample song, every section in view.
    Shot {
        name: "v01-sections",
        acts: &[Act::Reset, staged(0)],
    },
    // The Chorus is carried over the Verse: the span that moves glows and a line shows where it
    // will land.
    Shot {
        name: "v02-reorder-drag",
        acts: &[staged(1)],
    },
    Shot {
        name: "v03-reordered",
        acts: &[staged(2)],
    },
    // Alt: the Verse is copied, right after the original.
    Shot {
        name: "v04-copy-drag",
        acts: &[staged(3)],
    },
    Shot {
        name: "v05-copied",
        acts: &[staged(4)],
    },
    // The right-click of a section, and of the empty band beside the sections.
    Shot {
        name: "v06-section-menu",
        acts: &[Act::Reset, staged(10)],
    },
    Shot {
        name: "v07-band-menu",
        acts: &[Act::Key("escape"), staged(11)],
    },
    // The pencil on the band: the bars it covers are a new section named after a preset.
    Shot {
        name: "v08-pencil-paint",
        acts: &[Act::Key("escape"), staged(12)],
    },
    Shot {
        name: "v09-pencil-painted",
        acts: &[staged(13)],
    },
    // Time: the loop's right-click, then Ctrl+Alt+I puts the loop's length in at its head.
    Shot {
        name: "v10-loop-menu",
        acts: &[Act::Reset, staged(20)],
    },
    Shot {
        name: "v11-time-inserted",
        acts: &[Act::Key("escape"), staged(21)],
    },
    // The track list folded to a 24px strip with Ctrl+Shift+L.
    Shot {
        name: "v12-tracklist-folded",
        acts: &[Act::Reset, staged(30)],
    },
];

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Modifiers::default()
    }
}

fn down(
    p: (f32, f32),
    button: MouseButton,
    modifiers: Modifiers,
    window: &mut Window,
    cx: &mut App,
) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: None,
            modifiers,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: at(p),
            button,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn drag_to(p: (f32, f32), modifiers: Modifiers, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
        cx,
    );
}

fn up(p: (f32, f32), button: MouseButton, modifiers: Modifiers, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: at(p),
            button,
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

fn right_click(p: (f32, f32), window: &mut Window, cx: &mut App) {
    down(p, MouseButton::Right, Modifiers::default(), window, cx);
    up(p, MouseButton::Right, Modifiers::default(), window, cx);
}

fn key(stroke: &str, window: &mut Window, cx: &mut App) {
    window.dispatch_keystroke(Keystroke::parse(stroke).expect("a key"), cx);
}

fn bar_ticks(s: &Studio) -> Tick {
    Tick::from(s.project().ppq) * 4
}

/// The middle of a section's band, by name.
fn section_mid(s: &Studio, window: &Window, name: &str) -> (f32, f32) {
    let g = crate::screen::geometry(s, window);
    let sec = s
        .project()
        .sections
        .iter()
        .find(|x| x.name == name)
        .unwrap_or_else(|| panic!("no section {name}"))
        .clone();
    let x = (s.tick_x(&g, sec.start) + s.tick_x(&g, sec.start + sec.length)) / 2.0;
    (x, g.section_band.y + g.section_band.h / 2.0)
}

/// On the band, `bars` bars in from the start of the song (0 is bar 1).
fn band_at(s: &Studio, window: &Window, bars: i64) -> (f32, f32) {
    let g = crate::screen::geometry(s, window);
    (
        s.tick_x(&g, bars * bar_ticks(s)) + 4.0,
        g.section_band.y + g.section_band.h / 2.0,
    )
}

/// Just inside the Chorus's right end: a section carried here lands after the Chorus.
fn chorus_end(s: &Studio, window: &Window) -> (f32, f32) {
    let g = crate::screen::geometry(s, window);
    let chorus = s
        .project()
        .sections
        .iter()
        .find(|x| x.name == "Chorus")
        .expect("the chorus")
        .clone();
    (
        s.tick_x(&g, chorus.start + chorus.length) - 4.0,
        g.section_band.y + g.section_band.h / 2.0,
    )
}

fn start_of(s: &Studio, name: &str) -> Tick {
    s.project()
        .sections
        .iter()
        .find(|x| x.name == name)
        .map(|x| x.start)
        .expect("the section")
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            // The arrangement alone, every bar of the sample song in view.
            let mut s = studio.borrow_mut();
            s.drawer_open = false;
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
            s.scroll.0 = 0.0;
            s.beat_w = (g.lanes.w / 92.0).max(4.0);
            s.seek(0);
        }
        1 => {
            let (from, to) = {
                let s = studio.borrow();
                let verse = band_at(&s, window, start_of(&s, "Verse") / bar_ticks(&s));
                (section_mid(&s, window, "Chorus"), (verse.0, verse.1))
            };
            down(from, MouseButton::Left, Modifiers::default(), window, cx);
            drag_to(
                ((from.0 + to.0) / 2.0, from.1),
                Modifiers::default(),
                window,
                cx,
            );
            drag_to(to, Modifiers::default(), window, cx);
        }
        2 => {
            let to = {
                let s = studio.borrow();
                band_at(&s, window, start_of(&s, "Verse") / bar_ticks(&s))
            };
            up(to, MouseButton::Left, Modifiers::default(), window, cx);
            let s = studio.borrow();
            assert!(
                start_of(&s, "Chorus") < start_of(&s, "Verse"),
                "the chorus now comes before the verse"
            );
        }
        3 => {
            let (from, to) = {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                s.undo(&g);
                assert!(start_of(&s, "Verse") < start_of(&s, "Chorus"));
                (section_mid(&s, window, "Verse"), chorus_end(&s, window))
            };
            down(from, MouseButton::Left, alt(), window, cx);
            drag_to(((from.0 + to.0) / 2.0, from.1), alt(), window, cx);
            drag_to(to, alt(), window, cx);
        }
        4 => {
            let to = chorus_end(&studio.borrow(), window);
            let before = studio.borrow().project().sections.len();
            up(to, MouseButton::Left, alt(), window, cx);
            let s = studio.borrow();
            assert_eq!(s.project().sections.len(), before + 1, "a copy, not a move");
        }
        10 => {
            let p = section_mid(&studio.borrow(), window, "Verse");
            right_click(p, window, cx);
            assert!(studio.borrow().context_open(), "a section has a menu");
        }
        11 => {
            // Take the Chorus out so that the band has free bars, then right-click one.
            let p = {
                let mut s = studio.borrow_mut();
                let id = s
                    .project()
                    .sections
                    .iter()
                    .find(|x| x.name == "Chorus")
                    .expect("the chorus")
                    .id
                    .to_string();
                assert!(s.run_command("section.delete", json!({ "id": id })));
                let g = crate::screen::geometry(&s, window);
                s.rebuild(&g);
                band_at(&s, window, 13)
            };
            right_click(p, window, cx);
            assert!(studio.borrow().context_open(), "the empty band has a menu");
        }
        12 => {
            let from = {
                let mut s = studio.borrow_mut();
                s.tool = Tool::Draw;
                band_at(&s, window, 13)
            };
            let to = band_at(&studio.borrow(), window, 16);
            down(from, MouseButton::Left, Modifiers::default(), window, cx);
            drag_to(
                ((from.0 + to.0) / 2.0, from.1),
                Modifiers::default(),
                window,
                cx,
            );
            drag_to(to, Modifiers::default(), window, cx);
        }
        13 => {
            let to = band_at(&studio.borrow(), window, 16);
            let before = studio.borrow().project().sections.len();
            up(to, MouseButton::Left, Modifiers::default(), window, cx);
            let mut s = studio.borrow_mut();
            assert_eq!(s.project().sections.len(), before + 1, "a new section");
            s.tool = Tool::Select;
        }
        20 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.drawer_open = false;
                assert!(s.run_command("loop.set", json!({"start":"5.1","end":"7.1"})));
                let g = crate::screen::geometry(&s, window);
                (
                    s.tick_x(&g, 5 * bar_ticks(&s)),
                    g.ruler.y + crate::ruler::LOOP_H / 2.0,
                )
            };
            right_click(p, window, cx);
            assert!(studio.borrow().context_open(), "the loop has a menu");
        }
        21 => {
            let chorus = start_of(&studio.borrow(), "Chorus");
            key("ctrl-alt-i", window, cx);
            let s = studio.borrow();
            assert_eq!(start_of(&s, "Chorus"), chorus + 2 * bar_ticks(&s));
        }
        30 => {
            // The key as the window hands it on: the key and the character Shift makes of it.
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            let mods = crate::tools::Mods {
                ctrl: true,
                shift: true,
                alt: false,
            };
            assert!(
                s.mapped_key(&g, "l", Some("L"), mods),
                "Ctrl+Shift+L is bound"
            );
            assert!(s.side_size.folded, "the track list is a strip");
        }
        _ => unreachable!("arrange-v1 has stages 0 to 4, 10 to 13, 20, 21 and 30"),
    }
}
