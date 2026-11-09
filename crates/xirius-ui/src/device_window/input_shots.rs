//! Explicit parked-window scenarios through the actual device root listener.
use super::*;
use crate::shot::{Act, Play, Shot};
use serde_json::json;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "device-input-open",
        acts: &[Act::Stage(Play::Window(stage), 0)],
    },
    Shot {
        name: "device-input-shortcuts",
        acts: &[Act::Stage(Play::Window(stage), 1)],
    },
    Shot {
        name: "device-input-transport-save",
        acts: &[Act::Stage(Play::Window(stage), 2)],
    },
    Shot {
        name: "device-input-save-existing",
        acts: &[Act::Stage(Play::Window(stage), 3)],
    },
    Shot {
        name: "device-input-saved",
        acts: &[Act::Stage(Play::Window(stage), 4)],
    },
    Shot {
        name: "device-input-search",
        acts: &[Act::Stage(Play::Window(stage), 5)],
    },
    Shot {
        name: "device-narrow-resize",
        acts: &[Act::Stage(Play::Window(stage), 6)],
    },
    Shot {
        name: "device-narrow-capture",
        acts: &[Act::Stage(Play::Window(stage), 7)],
    },
    Shot {
        name: "device-wide-resize",
        acts: &[Act::Stage(Play::Window(stage), 8)],
    },
    Shot {
        name: "device-wide-capture",
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
