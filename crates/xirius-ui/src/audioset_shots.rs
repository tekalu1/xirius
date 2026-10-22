//! Audio/MIDI settings exercised through pointer events in the off-screen window.
use crate::settings::{Do, MenuKind, Section};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use gpui::{
    App, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PlatformInput, Window, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;
use xirius_engine::devices::{Device, Latency, Simulated};
use xirius_engine::record::AudioInput;

struct Signal;
impl AudioInput for Signal {
    fn channels(&self) -> usize {
        2
    }
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.025);
        right.fill(0.63);
    }
}

/// Deliberately unequal driver reports; none are computed from the mock's buffer formula.
pub fn fixture(s: &mut Studio) {
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    let devices = vec![
        Device {
            name: "Scarlett 2i2 (4th Gen)".into(),
            inputs: vec!["In 1".into(), "In 2".into()],
            outputs: (1..=4).map(|i| format!("Out {i}")).collect(),
        },
        Device {
            name: "USB Audio".into(),
            inputs: vec!["In 1".into()],
            outputs: vec!["Out 1".into(), "Out 2".into()],
        },
    ];
    let reports = [
        (32, (83, 101, 24)),
        (64, (113, 151, 24)),
        (128, (144, 158, 24)),
        (256, (290, 315, 24)),
        (512, (550, 570, 24)),
        (1024, (1080, 1100, 24)),
    ]
    .into_iter()
    .map(|(buffer, (input, output, conversion))| {
        (
            buffer,
            Latency {
                rate: 48000,
                input,
                output,
                conversion,
            },
        )
    })
    .collect();
    s.song
        .io
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(Box::new(Simulated::new(devices, reports, Box::new(Signal))));
    s.song
        .set(
            "audio.set",
            serde_json::json!({"key":"buffer","value":"128"}),
        )
        .unwrap();
    s.song
        .io
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .view
        .ports = vec![
        "Arturia KeyStep 37".into(),
        "nanoKONTROL2".into(),
        "loopMIDI Port".into(),
    ];
    s.poll_io();
}

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}
pub const AUDIOSET: &[Shot] = &[
    Shot {
        name: "a01-audio",
        acts: &[stage(0)],
    },
    Shot {
        name: "a02-device-menu",
        acts: &[stage(1)],
    },
    Shot {
        name: "a03-detected",
        acts: &[stage(2)],
    },
    Shot {
        name: "a04-buffer",
        acts: &[stage(3)],
    },
    Shot {
        name: "a05-midi",
        acts: &[stage(4)],
    },
    Shot {
        name: "a06-empty",
        acts: &[stage(5)],
    },
    Shot {
        name: "a07-audio-armed",
        acts: &[stage(6)],
    },
    Shot {
        name: "a08-midi-armed",
        acts: &[stage(7)],
    },
    Shot {
        name: "a09-output",
        acts: &[stage(8)],
    },
    Shot {
        name: "a10-output-selected",
        acts: &[stage(9)],
    },
    Shot {
        name: "a11-empty-before-drop",
        acts: &[stage(10)],
    },
    Shot {
        name: "a12-midi-dropped",
        acts: &[stage(11)],
    },
    Shot {
        name: "a13-midi-playing",
        acts: &[stage(12)],
    },
    Shot {
        name: "a14-driver",
        acts: &[stage(13)],
    },
    Shot {
        name: "a15-device-selected",
        acts: &[stage(14)],
    },
    Shot {
        name: "a16-input-and-offset",
        acts: &[stage(15)],
    },
    Shot {
        name: "a17-midi-switches",
        acts: &[stage(19), stage(16)],
    },
    Shot {
        name: "a18-midi-received",
        acts: &[stage(17)],
    },
    Shot {
        name: "a19-rail-latency",
        acts: &[stage(18)],
    },
];

fn key(key: &str, window: &mut Window, cx: &mut App) {
    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
}
fn press(studio: &Rc<RefCell<Studio>>, g: &Geometry, act: Do, window: &mut Window, cx: &mut App) {
    let p = item(&studio.borrow(), g, act);
    click(p, window, cx);
}
fn pick(studio: &Rc<RefCell<Studio>>, value: &str, window: &mut Window, cx: &mut App) {
    let p = {
        let s = studio.borrow();
        let menu = &s.settings.as_ref().unwrap().menu.as_ref().unwrap().menu;
        let cell = menu
            .layout
            .cells
            .iter()
            .find(|c| c.action == Some(crate::popup::Action::Pick(value.into())))
            .unwrap();
        (
            cell.rect.x + cell.rect.w / 2.0,
            cell.rect.y + cell.rect.h / 2.0,
        )
    };
    click(p, window, cx);
}
fn number(value: &str, window: &mut Window, cx: &mut App) {
    // The existing number field places its caret after the initial zero.
    key("backspace", window, cx);
    for c in value.chars() {
        key(&c.to_string(), window, cx);
    }
    key("enter", window, cx);
}

fn click(p: (f32, f32), window: &mut Window, cx: &mut App) {
    let position = point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

pub fn item(s: &Studio, g: &Geometry, act: Do) -> (f32, f32) {
    let scene = s.settings_scene(g);
    let i = scene.find(&act).expect("settings item");
    let p = (i.r.x + i.r.w / 2.0, i.r.y + i.r.h / 2.0);
    assert_eq!(scene.item_at(p.0, p.1).map(|i| &i.act), Some(&act));
    p
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let mut g = studio.borrow().geometry(
        f32::from(window.viewport_size().width),
        f32::from(window.viewport_size().height) - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H,
    );
    if n == 0 {
        fixture(&mut studio.borrow_mut());
        let p = studio.borrow().center_of(&g, Hit::RailSettings).unwrap();
        click(p, window, cx);
        let p = item(&studio.borrow(), &g, Do::Nav(Section::Audio));
        click(p, window, cx);
        assert_eq!(
            studio.borrow().settings.as_ref().unwrap().section,
            Section::Audio
        );
        return;
    }
    let act = match n {
        1 => Some(Do::Menu(MenuKind::Pref("audio.device".into()))),
        2 => {
            // Dismiss the menu by pressing its button again.
            let p = item(
                &studio.borrow(),
                &g,
                Do::Menu(MenuKind::Pref("audio.device".into())),
            );
            click(p, window, cx);
            Some(Do::Detect)
        }
        3 => Some(Do::Pref("audio.buffer".into(), "512".into())),
        4 => Some(Do::Nav(Section::Midi)),
        _ => None,
    };
    if let Some(act) = act {
        let p = item(&studio.borrow(), &g, act);
        click(p, window, cx);
        if n == 2 {
            let mut s = studio.borrow_mut();
            crate::shot::run_for(&mut s, 1.8, |_| false);
            s.poll_io();
            assert_eq!(s.song.io.lock().unwrap().view.selected, Some(1));
        }
        if n == 3 {
            assert_eq!(
                studio
                    .borrow()
                    .song
                    .io
                    .lock()
                    .unwrap()
                    .view
                    .latency
                    .unwrap()
                    .input,
                550
            );
        }
        return;
    }
    if n == 5 || n == 7 {
        let mut s = studio.borrow_mut();
        *s = Studio::new(crate::song::Song::empty());
        fixture(&mut s);
        g = s.geometry(g.size.0, g.size.1);
        s.resized(&g);
        if n == 5 {
            return;
        }
    }
    if n == 6 || n == 7 {
        let entry = if n == 6 {
            crate::start::Entry::Audio
        } else {
            crate::start::Entry::Midi
        };
        let p = studio.borrow().center_of(&g, Hit::Start(entry)).unwrap();
        click(p, window, cx);
        assert!(studio.borrow().project().tracks.values().any(|t| t.armed));
        if n == 6 {
            assert!(studio.borrow().drawer_open);
            assert_eq!(studio.borrow().tab, crate::studio::DrawerTab::Mixer);
        }
        if n == 7 {
            window.dispatch_event(
                PlatformInput::KeyDown(gpui::KeyDownEvent {
                    keystroke: Keystroke::parse("a").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            let mut s = studio.borrow_mut();
            assert!(s.transport.is_live());
            assert!(s.error.is_none(), "{:?}", s.error);
            crate::shot::run_for(&mut s, 0.3, |_| false);
            assert!(
                s.mixer
                    .meters
                    .get(xirius_session::host::Strip::Master)
                    .hold
                    .iter()
                    .any(|&v| v > 0.0)
            );
            drop(s);
            window.dispatch_event(
                PlatformInput::KeyUp(gpui::KeyUpEvent {
                    keystroke: Keystroke::parse("a").unwrap(),
                }),
                cx,
            );
        }
    }
    if n == 8 {
        let mut s = studio.borrow_mut();
        s.drawer_open = true;
        s.tab = crate::studio::DrawerTab::Mixer;
        let g = s.geometry(g.size.0, g.size.1);
        s.resized(&g);
        let hit = Hit::Mixer(crate::mixer::MixerHit::Output(
            xirius_core::id::TrackId::MASTER,
        ));
        let p = s.center_of(&g, hit).unwrap();
        drop(s);
        click(p, window, cx);
        assert!(studio.borrow().popup.is_some());
    }
    if n == 9 {
        let p = {
            let s = studio.borrow();
            let layout = s.popup.as_ref().unwrap().layout(g.size);
            let c = layout
                .cells
                .iter()
                .find(|c| c.action == Some(crate::popup::Action::Output("3:4".into())))
                .unwrap();
            (c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0)
        };
        click(p, window, cx);
        assert_eq!(studio.borrow().song.io.lock().unwrap().view.output, [2, 3]);
    }
    if n == 10 {
        let mut s = studio.borrow_mut();
        *s = Studio::new(crate::song::Song::empty());
        fixture(&mut s);
        s.resized(&g);
    }
    if n == 11 {
        // The clip is named after the file, so the file has the name a person would give it and
        // lives in a folder of its own.
        let dir = std::env::temp_dir().join(format!("xirius-audioset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Melody.mid");
        // Format 0: C4, one quarter note at 120 BPM.
        std::fs::write(
            &path,
            [
                77, 84, 104, 100, 0, 0, 0, 6, 0, 0, 0, 1, 1, 224, 77, 84, 114, 107, 0, 0, 0, 13, 0,
                144, 60, 100, 131, 96, 128, 60, 0, 0, 255, 47, 0,
            ],
        )
        .unwrap();
        let z = crate::start::zone(&g);
        let position = point(px(z.x + z.w / 2.0), px(z.y + z.h / 2.0 + Layout::TOP_BAR_H));
        let mut paths = gpui::ExternalPaths::default();
        paths.0.push(path.clone());
        window.dispatch_event(
            PlatformInput::FileDrop(gpui::FileDropEvent::Entered { position, paths }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::FileDrop(gpui::FileDropEvent::Submit { position }),
            cx,
        );
        window.dispatch_event(PlatformInput::FileDrop(gpui::FileDropEvent::Ended), cx);
        assert!(!studio.borrow().start_shown());
        let s = studio.borrow();
        let (track, clip) = s.selected_clip.expect("the dropped file is a clip");
        let project = s.project();
        let named = &project.tracks[&track]
            .clips
            .iter()
            .find(|c| c.id == clip)
            .unwrap()
            .name;
        assert_eq!(named, "Melody", "no folder or process in the title");
        drop(s);
        std::fs::remove_dir_all(dir).unwrap();
    }
    if n == 12 {
        key("space", window, cx);
        let mut s = studio.borrow_mut();
        assert!(s.transport.playing);
        assert!(s.error.is_none(), "{:?}", s.error);
        crate::shot::run_for(&mut s, 0.15, |_| false);
        assert!(
            s.mixer
                .meters
                .get(xirius_session::host::Strip::Master)
                .hold
                .iter()
                .any(|&v| v > 0.0)
        );
    }
    if n == 13 {
        key("space", window, cx);
        let p = studio.borrow().center_of(&g, Hit::RailSettings).unwrap();
        click(p, window, cx);
        press(studio, &g, Do::Nav(Section::Audio), window, cx);
        press(
            studio,
            &g,
            Do::Menu(MenuKind::Pref("audio.driver".into())),
            window,
            cx,
        );
        pick(studio, "wasapi-exclusive", window, cx);
        assert_eq!(
            studio
                .borrow()
                .song
                .settings()
                .lock()
                .unwrap()
                .prefs
                .text("audio.driver"),
            "wasapi-exclusive"
        );
    }
    if n == 14 {
        press(
            studio,
            &g,
            Do::Menu(MenuKind::Pref("audio.device".into())),
            window,
            cx,
        );
        pick(studio, "USB Audio", window, cx);
        assert_eq!(
            studio.borrow().song.io.lock().unwrap().view.device,
            "USB Audio"
        );
    }
    if n == 15 {
        press(
            studio,
            &g,
            Do::Menu(MenuKind::Pref("audio.device".into())),
            window,
            cx,
        );
        pick(studio, "Scarlett 2i2 (4th Gen)", window, cx);
        press(studio, &g, Do::Input(0), window, cx);
        key("down", window, cx);
        assert_eq!(
            studio.borrow().song.io.lock().unwrap().view.selected,
            Some(1)
        );
        press(
            studio,
            &g,
            Do::Field(crate::settings::FieldId::Pref("audio.record-offset".into())),
            window,
            cx,
        );
    }
    if n == 19 {
        number("-7.5", window, cx);
        assert_eq!(
            studio
                .borrow()
                .song
                .settings()
                .lock()
                .unwrap()
                .prefs
                .number("audio.record-offset"),
            -7.5
        );
    }
    if n == 16 {
        press(studio, &g, Do::Nav(Section::Midi), window, cx);
        press(
            studio,
            &g,
            Do::Pref("midi.inputs.nanoKONTROL2".into(), "false".into()),
            window,
            cx,
        );
        press(
            studio,
            &g,
            Do::Pref("midi.follow-selection".into(), "false".into()),
            window,
            cx,
        );
        press(
            studio,
            &g,
            Do::Field(crate::settings::FieldId::Pref("midi.input-offset".into())),
            window,
            cx,
        );
        let s = studio.borrow();
        let p = s.song.settings().lock().unwrap();
        assert!(!p.prefs.bool("midi.inputs.nanoKONTROL2"));
        assert!(!p.prefs.bool("midi.follow-selection"));
    }
    if n == 17 {
        number("4.5", window, cx);
        assert_eq!(
            studio
                .borrow()
                .song
                .settings()
                .lock()
                .unwrap()
                .prefs
                .number("midi.input-offset"),
            4.5
        );
        let mut s = studio.borrow_mut();
        let ports = s.song.io.lock().unwrap().view.ports.clone();
        // Repeated messages keep the activity visible for PrintWindow's capture delay.
        s.live_midi_input(Box::new(xirius_engine::record::MockMidi::with_ports(
            ports,
            (0..1000).map(|n| (n * 256, 0, [0xb0, 1, 64])).collect(),
        )));
        s.tick();
        assert!(
            s.song
                .io
                .lock()
                .unwrap()
                .view
                .received
                .contains(&"Arturia KeyStep 37".into())
        );
    }
    if n == 18 {
        press(studio, &g, Do::Close, window, cx);
        let p = studio.borrow().center_of(&g, Hit::RailSettings).unwrap();
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: point(px(p.0), px(p.1 + Layout::TOP_BAR_H)),
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        assert_eq!(studio.borrow().io_tooltip(), "入出力 6.8 ms");
    }
}

#[cfg(test)]
mod tests;
