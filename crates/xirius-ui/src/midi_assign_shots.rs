//! OPS-007: a controller's knob given to a part's fader through "MIDI 学習", and the list of
//! the assignments in the settings. The pointer goes through the offscreen window; the knob is
//! a feed the script pushes messages into (no device is opened).
use crate::mixer::MixerHit;
use crate::settings::{Do, Section};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Hit, Studio};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;
use xirius_engine::record::{MidiFeed, MidiSender};

thread_local! { static KNOB: RefCell<Option<MidiSender>> = const { RefCell::new(None) }; }

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-fader-menu",
        acts: &[Act::Reset, stage(0), stage(1)],
    },
    Shot {
        name: "02-learning",
        acts: &[stage(2)],
    },
    Shot {
        name: "03-assigned",
        acts: &[stage(3)],
    },
    Shot {
        name: "04-knob-moves-the-fader",
        acts: &[stage(4)],
    },
    Shot {
        name: "05-knob-taken",
        acts: &[stage(5), stage(6)],
    },
    Shot {
        name: "06-replaced",
        acts: &[stage(7)],
    },
    Shot {
        name: "07-settings-list",
        acts: &[stage(8)],
    },
    Shot {
        name: "08-freed",
        acts: &[stage(9)],
    },
];

fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    let p = point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: p,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    let p = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let layout = s.popup.as_ref().expect("menu").layout(g.size);
        let cell = layout
            .cells
            .iter()
            .find(|c| c.text == label)
            .unwrap_or_else(|| panic!("missing row {label}"));
        assert!(cell.live(), "disabled row {label}");
        (
            cell.rect.x + cell.rect.w / 2.0,
            cell.rect.y + cell.rect.h / 2.0,
        )
    };
    press(p, MouseButton::Left, window, cx);
}

fn track(s: &Studio, name: &str) -> xirius_core::id::TrackId {
    crate::arrange::probe::track(s, name)
}

fn fader_menu(studio: &Rc<RefCell<Studio>>, name: &str, window: &mut Window, cx: &mut App) {
    let p = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let r = s
            .mixer_rect(&g, MixerHit::Fader(track(&s, name)))
            .expect("the fader");
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    };
    press(p, MouseButton::Right, window, cx);
    assert!(studio.borrow().context_open());
}

/// The knob sends control change 21 at `value`; the screen reads it.
fn knob(studio: &Rc<RefCell<Studio>>, value: u8, window: &Window) {
    KNOB.with(|k| {
        k.borrow_mut()
            .as_mut()
            .expect("the feed")
            .send([0xb0, 21, value])
    });
    let mut s = studio.borrow_mut();
    let g = crate::screen::geometry(&s, window);
    s.poll_live_midi();
    s.run_midi_commands(&g);
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            let dir =
                std::env::temp_dir().join(format!("xirius-midi-shots-{}", std::process::id()));
            let mut s = studio.borrow_mut();
            s.song.load_settings(&dir);
            for name in ["Vocal", "Guitar", "Bass"] {
                s.song
                    .run("track.add.audio", json!({ "name": name }))
                    .unwrap();
            }
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            let (tx, rx) = MidiFeed::new(64);
            KNOB.with(|k| *k.borrow_mut() = Some(tx));
            s.live_midi_input(Box::new(rx));
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        1 => fader_menu(studio, "Vocal", window, cx),
        2 => choose(studio, "MIDI 学習", window, cx),
        3 => {
            assert!(studio.borrow().live_midi.assign.learning.is_some());
            knob(studio, 64, window);
            let s = studio.borrow();
            let v = track(&s, "Vocal");
            assert!(
                s.midimap()
                    .of_target(&xirius_prefs::midimap::Target::Volume(v.to_string()))
                    .is_some()
            );
        }
        4 => {
            for v in [40, 80, 110] {
                knob(studio, v, window);
            }
            let s = studio.borrow();
            let t = track(&s, "Vocal");
            assert!(s.mixer_value(MixerHit::Fader(t)).unwrap() > 0.0);
        }
        5 => {
            studio.borrow_mut().knob_end();
            fader_menu(studio, "Guitar", window, cx);
        }
        6 => {
            choose(studio, "MIDI 学習", window, cx);
            knob(studio, 64, window);
            assert!(studio.borrow().live_midi.assign.clash.is_some());
            assert!(studio.borrow().context_open());
        }
        7 => {
            choose(studio, "置き換える", window, cx);
            let s = studio.borrow();
            let (v, g) = (track(&s, "Vocal"), track(&s, "Guitar"));
            let map = s.midimap();
            use xirius_prefs::midimap::Target;
            assert!(map.of_target(&Target::Volume(v.to_string())).is_none());
            assert!(map.of_target(&Target::Volume(g.to_string())).is_some());
        }
        8 => {
            let at = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                s.center_of(&g, Hit::RailSettings).unwrap()
            };
            crate::keymap_shots::click(at, 1, window, cx);
            crate::keymap_shots::press(studio, Do::Nav(Section::Midi), window, cx);
        }
        _ => {
            let target = {
                let s = studio.borrow();
                let g = track(&s, "Guitar");
                format!("track.volume:{g}")
            };
            crate::keymap_shots::press(studio, Do::MidiUnset(target), window, cx);
            assert!(studio.borrow().midimap().is_empty());
        }
    }
}
