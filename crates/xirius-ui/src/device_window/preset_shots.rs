//! Explicit native editor scenarios crossing the host boundary to the shared list.
use super::*;
use crate::shot::{Act, Play, Shot};
use serde_json::json;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_COMMAND};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "preset-editor",
        acts: &[Act::Stage(Play::Window(stage), 0)],
    },
    Shot {
        name: "preset-request",
        acts: &[Act::Stage(Play::Window(stage), 1)],
    },
    Shot {
        name: "preset-search",
        acts: &[Act::Stage(Play::Window(stage), 2)],
    },
    Shot {
        name: "preset-selected",
        acts: &[Act::Stage(Play::Window(stage), 3)],
    },
    Shot {
        name: "preset-loaded",
        acts: &[Act::Stage(Play::Window(stage), 4)],
    },
    Shot {
        name: "preset-arrows",
        acts: &[Act::Stage(Play::Window(stage), 5)],
    },
    Shot {
        name: "preset-next-loaded",
        acts: &[Act::Stage(Play::Window(stage), 6)],
    },
    Shot {
        name: "preset-previous",
        acts: &[Act::Stage(Play::Window(stage), 7)],
    },
    Shot {
        name: "preset-previous-loaded",
        acts: &[Act::Stage(Play::Window(stage), 8)],
    },
    Shot {
        name: "preset-reopen",
        acts: &[Act::Stage(Play::Window(stage), 9)],
    },
    Shot {
        name: "preset-current-and-escape",
        acts: &[Act::Stage(Play::Window(stage), 10)],
    },
    Shot {
        name: "preset-save-as",
        acts: &[Act::Stage(Play::Window(stage), 11)],
    },
    Shot {
        name: "preset-save-name",
        acts: &[Act::Stage(Play::Window(stage), 12)],
    },
    Shot {
        name: "preset-save-submit",
        acts: &[Act::Stage(Play::Window(stage), 13)],
    },
    Shot {
        name: "preset-saved-file",
        acts: &[Act::Stage(Play::Window(stage), 14)],
    },
    Shot {
        name: "preset-open-request",
        acts: &[Act::Stage(Play::Window(stage), 15)],
    },
    Shot {
        name: "preset-overwrite",
        acts: &[Act::Stage(Play::Window(stage), 16)],
    },
    Shot {
        name: "preset-overwrite-saved",
        acts: &[Act::Stage(Play::Window(stage), 17)],
    },
    Shot {
        name: "preset-file-request",
        acts: &[Act::Stage(Play::Window(stage), 18)],
    },
    Shot {
        name: "preset-file-open",
        acts: &[Act::Stage(Play::Window(stage), 19)],
    },
    Shot {
        name: "preset-file-restored",
        acts: &[Act::Stage(Play::Window(stage), 20)],
    },
];

fn stage(n: u8, studio: &Rc<RefCell<Studio>>, _: &mut Window, cx: &mut App) {
    let names = if std::env::var_os("XIRIUS_PRESET_PROBE_LONG").is_some() {
        [
            "Clean 日本語の長いプリセット名と末尾まで続く設定",
            "Bright extended preset name with a long English description",
        ]
    } else {
        ["Clean", "Bright"]
    };
    if n == 0 {
        let mut s = studio.borrow_mut();
        prepare(&mut s);
        let id = crate::plugin_reload_shots::id(&s);
        let mode = std::env::var("XIRIUS_PRESET_PROBE_MODE").unwrap();
        assert!(xirius_core::model::PluginIsolation::NAMES.contains(&mode.as_str()));
        assert!(s.run_command("plugin.isolation.set", json!({"ids":[id], "mode":mode})));
        s.prepare_song();
        crate::shot::run_for(&mut s, 15.0, |s| !s.sound_preparing() && s.has_loaded(id));
        assert!(s.has_loaded(id), "{:?}", s.error);
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.inserts.iter().any(|p| p.id == id))
            .unwrap()
            .id;
        assert!(s.open_plugin(track, id));
        crate::shot::run_for(&mut s, 10.0, |s| s.editor_shown(id));
        assert!(s.editor_shown(id), "{:?}", s.error);
        for name in names {
            assert!(s.run_command(
                "preset.save",
                json!({"track":track,"of":id,"kind":"fx","name":name})
            ));
            crate::shot::run_for(&mut s, 10.0, |s| !s.input_blocked());
            assert!(s.error.is_none(), "{:?}", s.error);
        }
        crate::shot::run_for(&mut s, 10.0, |s| {
            s.instruments.presets.external.values().any(|names| {
                names.iter().any(|n| n.starts_with("Clean"))
                    && names.iter().any(|n| n.starts_with("Bright"))
            })
        });
        assert!(
            s.instruments
                .presets
                .external
                .values()
                .any(|names| names.iter().any(|n| n.starts_with("Clean")))
        );
        cx.refresh_windows();
        return;
    }
    let id = crate::plugin_reload_shots::id(&studio.borrow());
    if n == 1 {
        let hwnd = crate::plugin_keys_shots::frame(&studio.borrow());
        assert!(!hwnd.is_null());
        measure_bar(hwnd);
        capture(hwnd as isize, "native-preset-bar.bmp");
        // SAFETY: the frame belongs to this fixture's plugin or isolated host PID.
        assert_ne!(unsafe { PostMessageW(hwnd, WM_COMMAND, 2, 0) }, 0);
        crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| {
            s.mixer.preset_menu.is_some()
        });
        assert_eq!(studio.borrow().mixer.preset_menu.map(|r| r.1), Some(id));
        cx.refresh_windows();
        return;
    }
    if n == 4 {
        crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| {
            s.project()
                .tracks
                .values()
                .flat_map(|t| &t.inserts)
                .any(|p| p.id == id && p.preset.as_deref() == Some(names[0]))
        });
        assert!(
            studio
                .borrow()
                .project()
                .tracks
                .values()
                .flat_map(|t| &t.inserts)
                .any(|p| p.id == id && p.preset.as_deref() == Some(names[0]))
        );
        assert_eq!(
            studio.borrow().mixer.editor_bars[&id].presets,
            vec![
                "Default".to_string(),
                names[1].to_string(),
                names[0].to_string()
            ]
        );
        wait_preset(studio, id, names[0]);
        cx.refresh_windows();
        return;
    }
    if n == 5 {
        native_command(studio, 11);
        return;
    }
    if n == 6 {
        wait_preset(studio, id, "Default");
        cx.refresh_windows();
        return;
    }
    if n == 7 {
        native_command(studio, 8);
        return;
    }
    if n == 8 {
        wait_preset(studio, id, names[0]);
        cx.refresh_windows();
        return;
    }
    if n == 9 {
        native_command(studio, 2);
        return;
    }
    if n == 14 {
        wait_preset(studio, id, "Saved from preset window");
        crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| {
            s.browser
                .track_library
                .as_ref()
                .is_some_and(|rows| rows.iter().any(|r| r.name == "Saved from preset window"))
        });
        let s = studio.borrow();
        let rows = s.browser.track_library.as_ref().unwrap();
        let saved = rows
            .iter()
            .find(|r| r.name == "Saved from preset window")
            .unwrap();
        let file = std::path::Path::new(saved.file.as_ref().unwrap());
        assert!(std::fs::metadata(file).unwrap().len() > 0);
        drop(s);
        let mut s = studio.borrow_mut();
        assert!(s.run_command(
            "plugin.param.set",
            json!({"id":id,"param":"Gain","value":0.25})
        ));
        crate::shot::run_for(&mut s, 10.0, |s| !s.input_blocked());
        assert!(s.finish_preset_file(crate::studio::FileRequest::PresetSave(id), &portable_file()));
        crate::shot::run_for(&mut s, 10.0, |s| !s.input_blocked());
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(portable_file()).unwrap()).unwrap();
        assert_eq!(saved["params"]["Gain"], json!(0.25));
        assert!(s.run_command(
            "plugin.param.set",
            json!({"id":id,"param":"Gain","value":0.75})
        ));
        crate::shot::run_for(&mut s, 10.0, |s| !s.input_blocked());
        assert!(s.error.is_none(), "{:?}", s.error);
        return;
    }
    if n == 15 {
        native_command(studio, 2);
        return;
    }
    if n == 17 {
        wait_preset(studio, id, "Saved from preset window");
        let s = studio.borrow();
        let saved = s
            .browser
            .track_library
            .as_ref()
            .unwrap()
            .iter()
            .find(|r| r.name == "Saved from preset window")
            .unwrap();
        let data: serde_json::Value =
            serde_json::from_slice(&std::fs::read(saved.file.as_ref().unwrap()).unwrap()).unwrap();
        assert_eq!(data["devices"][0]["params"]["Gain"], json!(0.75));
        return;
    }
    if n == 18 {
        native_command(studio, 2);
        return;
    }
    if n == 20 {
        crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| {
            !s.input_blocked() && s.file_request.is_none()
        });
        let s = studio.borrow();
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(s.mixer.preset_file.is_none() && s.file_request.is_none());
        let p = s.project();
        let plugin = p
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|p| p.id == id)
            .unwrap();
        assert_eq!(plugin.params.get("Gain"), Some(&0.25));
        drop(s);
        studio.borrow_mut().close_editor(id);
        return;
    }
    let handle = cx
        .windows()
        .into_iter()
        .find(|h| {
            h.downcast::<DeviceWindow>()
                .and_then(|h| h.read(cx).ok())
                .is_some_and(|v| v.menu_only && v.id == Some(id) && Rc::ptr_eq(&v.studio, studio))
        })
        .expect("native request opened the shared preset surface");
    let view = handle.downcast::<DeviceWindow>().unwrap();
    if matches!(n, 11 | 16 | 19) {
        let label = match n {
            11 => "名前を付けて保存…",
            16 => "上書き保存",
            _ => "ファイルから読み込む…",
        };
        if n == 19 {
            studio.borrow_mut().mixer.preset_file = Some(portable_file());
        }
        for _ in 0..32 {
            let picked = {
                let pick = view
                    .read(cx)
                    .unwrap()
                    .popup
                    .as_ref()
                    .unwrap()
                    .pick
                    .as_ref()
                    .unwrap();
                pick.nav()[pick.selected].label == label
            };
            if picked {
                key(handle, "enter", cx);
                return;
            }
            key(handle, "down", cx);
        }
        panic!("preset footer {label} is keyboard reachable");
    }
    if n == 12 {
        view.update(cx, |view, window, cx| {
            let len = view.with(window, |s, _| {
                s.ask_field().unwrap().text().encode_utf16().count()
            });
            gpui::EntityInputHandler::replace_text_in_range(
                view,
                Some(0..len),
                "Saved from preset window",
                window,
                cx,
            );
            assert_eq!(
                view.with(window, |s, _| s.ask_field().unwrap().text().to_string()),
                "Saved from preset window"
            );
        })
        .unwrap();
        return;
    }
    if n == 13 {
        if std::env::var("XIRIUS_PRESET_PROBE_MODE").as_deref() == Ok("inprocess") {
            capture_popup(handle, cx, "shared-preset-save.bmp");
        }
        key(handle, "enter", cx);
        return;
    }
    if n == 10 {
        let current = view
            .read(cx)
            .unwrap()
            .popup
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap();
        assert!(current.nav().iter().any(|r| r.label == names[0] && r.on));
        handle
            .update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke::parse("escape").unwrap(),
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
            })
            .unwrap();
        assert!(view.read(cx).unwrap().popup.is_none());
        wait_preset(studio, id, names[0]);
        native_command(studio, 2);
        return;
    }
    if n == 2 {
        view.update(cx, |view, window, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "Clean", window, cx);
            assert_eq!(
                view.popup.as_ref().unwrap().pick.as_ref().unwrap().query,
                "Clean"
            );
        })
        .unwrap();
        return;
    }
    capture_popup(handle, cx, "shared-preset-list.bmp");
    for key in ["down", "up", "enter"] {
        handle
            .update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke::parse(key).unwrap(),
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
            })
            .unwrap();
    }
}

fn portable_file() -> std::path::PathBuf {
    let args: Vec<_> = std::env::args().collect();
    let root = args.windows(2).find(|a| a[0] == "--shot").unwrap();
    std::path::Path::new(&root[1]).join("round-trip.xirius-preset")
}

fn capture_popup(handle: gpui::AnyWindowHandle, cx: &mut App, name: &str) {
    let title = format!("Xirius capture {name}");
    handle
        .update(cx, |_, window, _| window.set_window_title(&title))
        .unwrap();
    capture(crate::park::own_handle(&title).unwrap(), name);
}

fn key(handle: gpui::AnyWindowHandle, key: &str, cx: &mut App) {
    handle
        .update(cx, |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse(key).unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
        })
        .unwrap();
}

fn native_command(studio: &Rc<RefCell<Studio>>, command: usize) {
    let hwnd = crate::plugin_keys_shots::frame(&studio.borrow());
    assert!(!hwnd.is_null());
    // SAFETY: the fixture resolved this process's or host's editor frame.
    assert_ne!(
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) },
        0
    );
    capture(hwnd as isize, &format!("before-command-{command}.bmp"));
    // SAFETY: these are the fixture frame's controls, not a desktop-wide input.
    assert_ne!(
        unsafe {
            windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(
                windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, command as i32),
            )
        },
        0,
        "native control {command} is enabled"
    );
    // SAFETY: the frame is resolved from this fixture's plugin and host PID.
    assert_ne!(unsafe { PostMessageW(hwnd, WM_COMMAND, command, 0) }, 0);
}

fn wait_preset(studio: &Rc<RefCell<Studio>>, id: PluginId, name: &str) {
    let matches = |s: &Studio| {
        s.project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .any(|p| p.id == id && p.preset.as_deref() == Some(name))
    };
    crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| {
        matches(s)
            && !s.input_blocked()
            && !s.sound_preparing()
            && s.editor_shown(id)
            && s.transport
                .host()
                .is_some_and(|h| !h.swap_pending() && !h.plugins().late_pending())
    });
    studio.borrow_mut().tick();
    let s = studio.borrow();
    assert!(!s.input_blocked() && !s.sound_preparing() && s.editor_shown(id));
    assert!(
        s.transport
            .host()
            .is_some_and(|h| !h.swap_pending() && !h.plugins().late_pending())
    );
    assert!(
        matches(&s),
        "preset {name}; bar={:?}; error={:?}; blocked={}",
        s.mixer.editor_bars.get(&id),
        s.error,
        s.input_blocked()
    );
}

fn capture(hwnd: isize, name: &str) {
    assert!(!crate::park::is_foreground(hwnd));
    let args: Vec<_> = std::env::args().collect();
    let root = args.windows(2).find(|a| a[0] == "--shot").unwrap();
    let path = std::path::Path::new(&root[1]).join(name);
    let captured = xirius_vst3::capture_window(hwnd, &path).unwrap();
    assert!(captured.distinct_colors > 2);
}

fn measure_bar(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::{
        Foundation::{POINT, RECT},
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{GetClientRect, GetDlgItem, GetWindowRect},
    };
    let mut client = RECT::default();
    let mut origin = POINT::default();
    let mut controls = Vec::new();
    // SAFETY: the frame and its child controls belong to this run's editor.
    unsafe {
        assert_ne!(GetClientRect(hwnd, &mut client), 0);
        assert_ne!(ClientToScreen(hwnd, &mut origin), 0);
        for id in [1, 8, 2, 11, 6, 4, 5] {
            let mut r = RECT::default();
            let child = GetDlgItem(hwnd, id);
            assert!(!child.is_null());
            assert_ne!(GetWindowRect(child, &mut r), 0);
            controls.push((id, r.left - origin.x, r.right - origin.x));
        }
    }
    for pair in controls.windows(2) {
        assert!(pair[0].2 <= pair[1].1, "{controls:?}");
    }
    assert!(
        controls
            .iter()
            .all(|r| r.1 >= 0 && r.2 <= client.right && r.2 > r.1)
    );
    let group_twice_center = controls[1].1 + controls[3].2;
    assert!(
        (group_twice_center - client.right).abs() <= 2,
        "{controls:?}"
    );
    let args: Vec<_> = std::env::args().collect();
    let root = args.windows(2).find(|a| a[0] == "--shot").unwrap();
    std::fs::write(std::path::Path::new(&root[1]).join("native-bar-bounds.json"),
        serde_json::to_vec_pretty(&json!({"client_width":client.right,"group_twice_center":group_twice_center,"controls":controls})).unwrap()).unwrap();
}

fn prepare(s: &mut Studio) {
    let Some(path) = std::env::var_os("XIRIUS_PRESET_PROBE_PLUGIN") else {
        crate::plugin_reload_shots::prepare(s, 0);
        return;
    };
    let entries = xirius_plugins::probe(std::path::Path::new(&path)).unwrap();
    let entry = entries.first().unwrap();
    let project = s.project();
    let track = project.tracks.values().find(|t| t.name == "Vocal").unwrap();
    let ids: Vec<_> = track.inserts.iter().map(|p| p.id).collect();
    if !ids.is_empty() {
        assert!(s.run_command("plugin.remove", json!({"ids":ids})));
    }
    assert!(s.run_command(
        "plugin.add",
        json!({
            "track":track.id, "name":entry.name, "kind":"effect", "format":"vst3", "uid":entry.uid
        })
    ));
    s.use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Mixer;
}
