//! Explicit parked-window scenarios through the actual device root listener.
use super::*;
use crate::shot::{Act, Play, Shot};
use serde_json::json;

pub const KEYBOARD_SHOTS: &[Shot] = &[
    Shot {
        name: "keyboard-01-zones-open",
        acts: &[Act::Stage(Play::Window(zone_stage), 0)],
    },
    Shot {
        name: "keyboard-02-zones-left",
        acts: &[Act::Stage(Play::Window(zone_stage), 1)],
    },
    Shot {
        name: "keyboard-03-zones-right",
        acts: &[Act::Stage(Play::Window(zone_stage), 2)],
    },
    Shot {
        name: "keyboard-04-zones-end",
        acts: &[Act::Stage(Play::Window(zone_stage), 3)],
    },
    Shot {
        name: "keyboard-05-zones-capture",
        acts: &[Act::Stage(Play::Window(zone_stage), 4)],
    },
    Shot {
        name: "keyboard-06-open",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 0)],
    },
    Shot {
        name: "keyboard-07-drag",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 1)],
    },
    Shot {
        name: "keyboard-08-midi",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 2)],
    },
    Shot {
        name: "keyboard-09-close",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 3)],
    },
    Shot {
        name: "keyboard-10-multi-open",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 4)],
    },
    Shot {
        name: "keyboard-11-multi-play",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 5)],
    },
    Shot {
        name: "keyboard-12-main-open",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 6)],
    },
    Shot {
        name: "keyboard-13-main-blur",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 7)],
    },
    Shot {
        name: "keyboard-14-main-released",
        acts: &[Act::Stage(Play::Window(keyboard_stage), 8)],
    },
];

fn zone_stage(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if n == 0 {
        stage(0, studio, window, cx);
    }
    let track = studio
        .borrow()
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    let id = crate::instrument::current(&studio.borrow().project().tracks[&track])
        .unwrap()
        .id;
    if n == 0 {
        let mut s = studio.borrow_mut();
        for name in ["Sub_C0.wav", "Piano_C2.wav", "Piano_C4.wav", "Bell_C8.wav"] {
            assert!(s.run_command(
                "plugin.samples.add",
                json!({"id":id,"file":std::env::current_dir().unwrap().join(name)})
            ));
            crate::shot::run_for(&mut s, 10.0, |s| !s.input_blocked());
        }
        cx.refresh_windows();
        return;
    }
    let h = handle(studio, id, cx);
    if n == 1 {
        h.update(cx, |_, w, _| w.resize(size(px(520.0), px(500.0))))
            .unwrap();
        return;
    }
    if n == 3 {
        capture_keyboard(h, "keyboard-zones-left-native.bmp", cx);
    }
    if n == 4 {
        capture_keyboard(h, "keyboard-zones-right-native.bmp", cx);
        return;
    }
    let keyboard = h
        .downcast::<DeviceWindow>()
        .unwrap()
        .update(cx, |v, w, _| {
            v.with(w, |s, g| {
                let p = s.project();
                let i = s
                    .instruments
                    .open
                    .iter()
                    .position(|w| w.plugin == id)
                    .unwrap();
                let (_, x, def) =
                    crate::instrument::window::live(&p, &s.instruments.open[i]).unwrap();
                s.instruments
                    .body(s.instruments.frame(i, g), x, def)
                    .keyboard
                    .unwrap()
            })
        })
        .unwrap();
    let thumb = keyboard.thumb();
    pointer(h, (thumb.x + 2.0, thumb.y + 2.0), 0, cx);
    pointer(
        h,
        (
            if n == 2 {
                keyboard.view.x + 2.0
            } else {
                keyboard.view.right() - 1.0
            },
            thumb.y + 2.0,
        ),
        1,
        cx,
    );
    let expected = if n == 2 {
        0.0
    } else {
        keyboard.extent - keyboard.view.w
    };
    assert!((studio.borrow().instruments.keyboard_scroll[&id] - expected).abs() < 0.1);
    pointer(h, (thumb.x, thumb.y), 2, cx);
}

#[cfg(windows)]
fn main_activation(active: bool) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
    }
    let hwnd = crate::park::own_handle("Xirius").unwrap();
    assert!(!crate::park::is_foreground(hwnd));
    // SAFETY: this process's parked test window only. A queued activation
    // message exercises the native adapter without changing OS foreground.
    assert_ne!(unsafe { PostMessageW(hwnd, 6, usize::from(active), 0) }, 0);
}

fn pointer(handle: gpui::AnyWindowHandle, at: (f32, f32), action: u8, cx: &mut App) {
    handle
        .update(cx, |_, window, cx| {
            let position = point(px(at.0), px(at.1));
            let modifiers = gpui::Modifiers::default();
            let event = match action {
                0 => gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                }),
                1 => gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position,
                    modifiers,
                    pressed_button: Some(MouseButton::Left),
                }),
                _ => gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                }),
            };
            window.dispatch_event(event, cx);
        })
        .unwrap();
}

fn keyboard_stage(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if n == 0 {
        let id = {
            let mut s = studio.borrow_mut();
            s.instruments.keyboard_scroll.clear();
            s.instruments.open[0].plugin
        };
        handle(studio, id, cx)
            .update(cx, |_, w, _| w.resize(size(px(780.0), px(500.0))))
            .unwrap();
        cx.refresh_windows();
        return;
    }
    let track = studio
        .borrow()
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    let id = crate::instrument::current(&studio.borrow().project().tracks[&track])
        .unwrap()
        .id;
    if n >= 6 {
        if n == 6 {
            let mut s = studio.borrow_mut();
            s.select_row(Some(crate::tree::RowKind::Track(track)));
            s.live_midi_input(Box::new(xirius_engine::record::MockMidi::with_ports(
                vec!["Keys".into()],
                vec![(0, 0, [0x90, 48, 100])],
            )));
            s.poll_live_midi();
            #[cfg(windows)]
            main_activation(true);
            cx.refresh_windows();
            return;
        }
        if n == 7 {
            let at = {
                let s = studio.borrow();
                let g = s.geometry(1280.0, 900.0);
                let keyboard = s.channel_view(&g).unwrap().keyboard.unwrap();
                let k = keyboard.keys.iter().find(|k| k.pitch == 48).unwrap();
                point(px(k.r.x + k.r.w / 2.0), px(k.r.bottom() - 5.0))
            };
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: MouseButton::Left,
                    position: at,
                    modifiers: gpui::Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            assert!(
                studio
                    .borrow()
                    .channel
                    .as_ref()
                    .unwrap()
                    .mouse_note
                    .is_some()
            );
            #[cfg(windows)]
            main_activation(false);
            return;
        }
        let mut s = studio.borrow_mut();
        assert!(
            s.channel.as_ref().unwrap().mouse_note.is_none(),
            "native main blur releases its mouse"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            s.tick();
            s.transport.poll();
            if s.transport
                .host()
                .is_some_and(|h| h.plugins().multi_pressed_keys(track).contains(&48))
                && s.transport
                    .take_levels()
                    .get(&xirius_session::host::Strip::Master)
                    .is_some_and(|v| v.peak.iter().any(|p| *p > 0.001))
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "MIDI PCM must survive native blur"
            );
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        assert!(
            s.transport
                .host()
                .unwrap()
                .plugins()
                .multi_pressed_keys(track)
                .contains(&48)
        );
        s.release_midi();
        return;
    }
    if n == 4 {
        let mut s = studio.borrow_mut();
        s.release_midi();
        assert!(s.run_command("rack.layer.add", json!({"track":track,"name":"Sampler"})));
        s.open_channel(track);
        cx.refresh_windows();
        return;
    }
    if n == 5 {
        let h = cx
            .windows()
            .into_iter()
            .find(|h| {
                h.downcast::<DeviceWindow>()
                    .and_then(|h| h.read(cx).ok())
                    .is_some_and(|v| v.id.is_none() && Rc::ptr_eq(&v.studio, studio))
            })
            .unwrap();
        let at = h
            .downcast::<DeviceWindow>()
            .unwrap()
            .update(cx, |view, window, _| {
                view.with(window, |s, g| {
                    let keyboard = s.channel_view(g).unwrap().keyboard.unwrap();
                    let k = keyboard.keys.iter().find(|k| k.pitch == 48).unwrap();
                    (k.r.x + k.r.w / 2.0, k.r.bottom() - 5.0)
                })
            })
            .unwrap();
        pointer(h, at, 0, cx);
        assert_eq!(
            studio
                .borrow()
                .channel
                .as_ref()
                .unwrap()
                .mouse_note
                .unwrap()
                .0,
            48
        );
        pointer(h, at, 2, cx);
        assert!(
            studio
                .borrow()
                .channel
                .as_ref()
                .unwrap()
                .mouse_note
                .is_none()
        );
        capture_keyboard(h, "keyboard-multi-native.bmp", cx);
        return;
    }
    let h = handle(studio, id, cx);
    let at = |part, cx: &mut App| {
        h.downcast::<DeviceWindow>()
            .unwrap()
            .update(cx, |view, window, _| {
                view.with(window, |s, g| s.instrument_center(g, id, part).unwrap())
            })
            .unwrap()
    };
    if n == 1 {
        let first = at(crate::instrument::window::Part::Key(60), cx);
        let next = at(crate::instrument::window::Part::Key(61), cx);
        pointer(h, first, 0, cx);
        assert_eq!(studio.borrow().instruments.note.unwrap().pitch, 60);
        pointer(h, first, 1, cx);
        pointer(h, next, 1, cx);
        assert_eq!(studio.borrow().instruments.note.unwrap().pitch, 61);
        pointer(h, (1.0, 80.0), 1, cx);
        assert!(studio.borrow().instruments.note.is_none());
        pointer(h, next, 2, cx);
        return;
    }
    if n == 2 {
        let mut s = studio.borrow_mut();
        s.select_row(Some(crate::tree::RowKind::Track(track)));
        s.live_midi_input(Box::new(xirius_engine::record::MockMidi::with_ports(
            vec!["Keys".into()],
            vec![(0, 0, [0x90, 60, 100])],
        )));
        s.poll_live_midi();
        crate::shot::run_for(&mut s, 10.0, |s| {
            s.transport
                .host()
                .is_some_and(|h| h.plugins().pressed_keys(id).contains(&60))
        });
        s.poll_probes();
        assert!(
            s.instruments
                .midi_pressed
                .get(&id)
                .is_some_and(|k| k.contains(&60)),
            "{:?}",
            s.error
        );
        drop(s);
        let key_at = at(crate::instrument::window::Part::Key(62), cx);
        pointer(h, key_at, 0, cx);
        key(studio, id, "escape", cx);
        assert!(studio.borrow().instruments.note.is_none());
        capture_keyboard(h, "keyboard-sampler-native.bmp", cx);
        return;
    }
    capture_keyboard(h, "keyboard-sampler-native.bmp", cx);
    let key_at = at(crate::instrument::window::Part::Key(62), cx);
    pointer(h, key_at, 0, cx);
    let close = at(crate::instrument::window::Part::Close, cx);
    pointer(h, close, 0, cx);
    assert!(studio.borrow().instruments.note.is_none());
    assert!(!studio.borrow().instruments.is_open(id));
    assert_eq!(
        studio
            .borrow()
            .transport
            .host()
            .unwrap()
            .plugins()
            .pressed_keys(id),
        vec![60]
    );
}

fn capture_keyboard(h: gpui::AnyWindowHandle, name: &str, cx: &mut App) {
    h.update(cx, |_, w, _| w.set_window_title("Xirius keyboard capture"))
        .unwrap();
    #[cfg(target_os = "windows")]
    {
        let hwnd = crate::park::own_handle("Xirius keyboard capture").unwrap();
        assert!(!crate::park::is_foreground(hwnd));
        let args: Vec<_> = std::env::args().collect();
        let root = args.windows(2).find(|a| a[0] == "--shot").unwrap();
        xirius_vst3::capture_window(hwnd, &std::path::Path::new(&root[1]).join(name)).unwrap();
    }
    #[cfg(not(target_os = "windows"))]
    let _ = name;
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "device-input-01-open",
        acts: &[Act::Stage(Play::Window(stage), 0)],
    },
    Shot {
        name: "device-input-02-shortcuts",
        acts: &[Act::Stage(Play::Window(stage), 1)],
    },
    Shot {
        name: "device-input-03-transport-save",
        acts: &[Act::Stage(Play::Window(stage), 2)],
    },
    Shot {
        name: "device-input-04-save-existing",
        acts: &[Act::Stage(Play::Window(stage), 3)],
    },
    Shot {
        name: "device-input-05-saved",
        acts: &[Act::Stage(Play::Window(stage), 4)],
    },
    Shot {
        name: "device-input-06-search",
        acts: &[Act::Stage(Play::Window(stage), 5)],
    },
    Shot {
        name: "device-input-07-narrow-resize",
        acts: &[Act::Stage(Play::Window(stage), 6)],
    },
    Shot {
        name: "device-input-08-narrow-capture",
        acts: &[Act::Stage(Play::Window(stage), 7)],
    },
    Shot {
        name: "device-input-09-wide-resize",
        acts: &[Act::Stage(Play::Window(stage), 8)],
    },
    Shot {
        name: "device-input-10-wide-capture",
        acts: &[Act::Stage(Play::Window(stage), 9)],
    },
];

fn handle(studio: &Rc<RefCell<Studio>>, id: PluginId, cx: &App) -> gpui::AnyWindowHandle {
    cx.windows()
        .into_iter()
        .find(|handle| {
            handle
                .downcast::<DeviceWindow>()
                .and_then(|h| h.read(cx).ok())
                .is_some_and(|view| {
                    view.id == Some(id) && Rc::ptr_eq(&view.studio, studio) && !view.menu_only
                })
        })
        .expect("the independent device window")
}

fn key(studio: &Rc<RefCell<Studio>>, id: PluginId, value: &str, cx: &mut App) {
    // AnyWindowHandle updates the window without borrowing its root entity.
    // Dispatch then reaches the same listener used by platform keyboard input.
    handle(studio, id, cx)
        .update(cx, |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse(value).unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
        })
        .unwrap();
}

fn stage(n: u8, studio: &Rc<RefCell<Studio>>, _: &mut Window, cx: &mut App) {
    let track = studio
        .borrow()
        .project()
        .tracks
        .values()
        .find(|t| {
            matches!(
                t.name.as_str(),
                "Keys" | "Keyboard target" | "Saved from device"
            )
        })
        .unwrap()
        .id;
    if n == 0 {
        let mut s = studio.borrow_mut();
        s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../temporary/plugin-ux-impl/device-input-run")
            .join(format!("song-{}", std::process::id()));
        assert!(s.run_command("project.save-as", json!({"path":destination})));
        crate::shot::run_for(&mut s, 15.0, |s| !s.input_blocked());
        assert!(s.song.is_saved(), "{:?}", s.error);
        for file in ["vocal_take1.wav", "guitar_di.wav"] {
            std::fs::copy(file, destination.join(file)).unwrap();
        }
        assert!(s.set_instrument(
            track,
            &crate::instrument::Pick::built_in(crate::instrument::DEFAULT)
        ));
        let id = crate::instrument::current(&s.project().tracks[&track])
            .unwrap()
            .id;
        assert!(s.run_command(
            "plugin.samples.add",
            json!({
                "id":id, "file":destination.join("vocal_take1.wav")
            })
        ));
        crate::shot::run_for(&mut s, 10.0, |s| !s.input_blocked());
        s.show_instrument(track, id);
        cx.refresh_windows();
        return;
    }
    let id = crate::instrument::current(&studio.borrow().project().tracks[&track])
        .unwrap()
        .id;
    let viewport = studio.borrow().viewport_size.get();
    if n >= 6 {
        let handle = handle(studio, id, cx);
        if n == 6 || n == 8 {
            handle
                .update(cx, |_, window, _| {
                    window.set_window_title("Xirius builtin capture");
                    window.resize(size(px(if n == 6 { 360.0 } else { 1280.0 }), px(400.0)));
                })
                .unwrap();
        } else {
            #[cfg(target_os = "windows")]
            {
                let hwnd = crate::park::own_handle("Xirius builtin capture").unwrap();
                assert!(!crate::park::is_foreground(hwnd));
                let args: Vec<_> = std::env::args().collect();
                let root = args.windows(2).find(|a| a[0] == "--shot").unwrap();
                let path = std::path::Path::new(&root[1]).join(if n == 7 {
                    "builtin-narrow.bmp"
                } else {
                    "builtin-wide.bmp"
                });
                assert!(
                    xirius_vst3::capture_window(hwnd, &path)
                        .unwrap()
                        .distinct_colors
                        > 2
                );
            }
        }
        assert_eq!(studio.borrow().viewport_size.get(), viewport);
        return;
    }
    if n == 5 {
        let name = "Main field stays unchanged";
        studio.borrow_mut().input_map.name = Some(crate::track_name::Name {
            track,
            field: crate::settings::Field::new(crate::settings::FieldId::Search, name.into()),
        });
        for step in 0..48 {
            key(studio, id, "tab", cx);
            assert_eq!(
                studio.borrow().instruments.focus.map(|f| f.0),
                Some(id),
                "Tab step {step}"
            );
        }
        let view = handle(studio, id, cx).downcast::<DeviceWindow>().unwrap();
        view.update(cx, |view, window, cx| {
            view.with(window, |s, _| {
                s.open_inst_menu(
                    crate::instrument::menu::Menu::Preset(track, id),
                    crate::studio::Rect::new(100.0, 20.0, 220.0, 28.0),
                );
            });
            gpui::EntityInputHandler::replace_text_in_range(
                view,
                None,
                "No matching preset",
                window,
                cx,
            );
            assert_eq!(
                view.popup.as_ref().unwrap().pick.as_ref().unwrap().query,
                "No matching preset"
            );
        })
        .unwrap();
        key(studio, id, "space", cx);
        assert!(!studio.borrow().transport.playing);
        key(studio, id, "tab", cx);
        key(studio, id, "escape", cx);
        assert!(
            view.read(cx)
                .unwrap()
                .popup
                .as_ref()
                .unwrap()
                .pick
                .as_ref()
                .unwrap()
                .query
                .is_empty()
        );
        key(studio, id, "down", cx);
        key(studio, id, "up", cx);
        key(studio, id, "escape", cx);
        assert!(view.read(cx).unwrap().popup.is_none());
        assert_eq!(
            studio
                .borrow()
                .input_map
                .name
                .as_ref()
                .unwrap()
                .field
                .text(),
            name
        );
        assert_eq!(studio.borrow().viewport_size.get(), viewport);
        studio.borrow_mut().input_map.name = None;
        return;
    }
    if n >= 3 {
        crate::shot::run_for(&mut studio.borrow_mut(), 15.0, |s| !s.input_blocked());
        assert!(studio.borrow().song.is_saved());
        assert!(
            studio.borrow().error.is_none(),
            "{:?}",
            studio.borrow().error
        );
        if n == 3 {
            assert!(studio.borrow_mut().run_command(
                "track.rename",
                json!({"id":track,"name":"Saved from device"})
            ));
            key(studio, id, "ctrl-s", cx);
            assert!(studio.borrow().saving.is_some());
        } else {
            let saved =
                std::fs::read_to_string(studio.borrow().song.dir().unwrap().join("project.json"))
                    .unwrap();
            assert!(saved.contains("Saved from device"));
            assert_eq!(studio.borrow().viewport_size.get(), viewport);
        }
        return;
    }
    if n == 1 {
        let before = studio.borrow().project().tracks[&track].name.clone();
        assert!(
            studio
                .borrow_mut()
                .run_command("track.rename", json!({"id":track,"name":"Keyboard target"}))
        );
        key(studio, id, "ctrl-z", cx);
        crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| !s.input_blocked());
        assert_eq!(studio.borrow().project().tracks[&track].name, before);
        key(studio, id, "ctrl-shift-z", cx);
        crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| !s.input_blocked());
        assert_eq!(
            studio.borrow().project().tracks[&track].name,
            "Keyboard target"
        );
        assert_eq!(studio.borrow().viewport_size.get(), viewport);
        return;
    }
    studio.borrow_mut().tick();
    crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| !s.input_blocked());
    assert!(!studio.borrow().input_blocked());
    // The silent script does not prepare an output at startup. Open the same
    // asynchronous song output used by the app before testing transport keys.
    studio.borrow_mut().prepare_song();
    crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| {
        !s.sound_preparing() && s.transport.host().is_some()
    });
    assert!(studio.borrow().transport.host().is_some());
    assert!(!studio.borrow().sound_preparing());
    key(studio, id, "space", cx);
    crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| s.transport.playing);
    assert!(
        studio.borrow().transport.playing,
        "error={:?} blocked={} preparing={} saved={}",
        studio.borrow().error,
        studio.borrow().input_blocked(),
        studio.borrow().sound_preparing(),
        studio.borrow().song.is_saved()
    );
    key(studio, id, "space", cx);
    assert!(!studio.borrow().transport.playing);
    key(studio, id, "ctrl-s", cx);
    assert!(studio.borrow().saving.is_some());
    assert_eq!(studio.borrow().viewport_size.get(), viewport);
    eprintln!("device-input shortcuts=true viewport=true save-request=true");
}
