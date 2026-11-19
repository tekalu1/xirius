//! UI-337: measure the native popup, including its actual desktop stacking order.
use super::*;
use crate::shot::{Act, Play, Shot};
use serde_json::json;
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::ClientToScreen,
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::WindowsAndMessaging::*,
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "front-01-editor",
        acts: &[Act::Stage(Play::Window(stage), 0)],
    },
    Shot {
        name: "front-02-request",
        acts: &[Act::Stage(Play::Window(stage), 1)],
    },
    Shot {
        name: "front-03-select",
        acts: &[Act::Stage(Play::Window(stage), 2)],
    },
    Shot {
        name: "front-04-loaded",
        acts: &[Act::Stage(Play::Window(stage), 3)],
    },
];

type Timings = (Instant, serde_json::Map<String, serde_json::Value>);

thread_local! {
    static TIMES: RefCell<Option<Timings>> = const { RefCell::new(None) };
}

pub(crate) fn mark(name: &str) {
    TIMES.with_borrow_mut(|times| {
        if let Some((start, values)) = times {
            values
                .entry(name.to_owned())
                .or_insert_with(|| json!(start.elapsed().as_secs_f64() * 1000.0));
        }
    });
}

fn foreground(frame: HWND) {
    // SAFETY: the explicit desktop scenario only. Remove the parking-only activation
    // restriction, temporarily share the foreground queue, and never inject input.
    unsafe {
        SetWindowLongW(
            frame,
            GWL_EXSTYLE,
            GetWindowLongW(frame, GWL_EXSTYLE) & !(WS_EX_NOACTIVATE as i32),
        );
        let own = GetCurrentThreadId();
        let active = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let attached = active != 0 && active != own && AttachThreadInput(own, active, 1) != 0;
        let target = GetWindowThreadProcessId(frame, std::ptr::null_mut());
        let target_attached = target != own && AttachThreadInput(own, target, 1) != 0;
        SetForegroundWindow(frame);
        BringWindowToTop(frame);
        let mut result = 0;
        SendMessageTimeoutW(frame, WM_NULL, 0, 0, SMTO_ABORTIFHUNG, 2_000, &mut result);
        let deadline = Instant::now() + Duration::from_secs(2);
        while GetForegroundWindow() != frame && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if target_attached {
            AttachThreadInput(own, target, 0);
        }
        if attached {
            AttachThreadInput(own, active, 0);
        }
        assert_eq!(
            GetForegroundWindow(),
            frame,
            "editor must be foreground before BM_CLICK"
        );
    }
}

fn output() -> std::path::PathBuf {
    let args: Vec<_> = std::env::args().collect();
    let dir = &args.windows(2).find(|a| a[0] == "--shot").unwrap()[1];
    std::path::Path::new(dir).join("front.json")
}

fn handle(studio: &Rc<RefCell<Studio>>, cx: &App) -> Option<WindowHandle<DeviceWindow>> {
    cx.windows().into_iter().find_map(|h| {
        let h = h.downcast::<DeviceWindow>()?;
        h.read(cx)
            .ok()
            .filter(|v| v.menu_only && Rc::ptr_eq(&v.studio, studio))?;
        Some(h)
    })
}

fn topmost(hwnd: HWND) -> bool {
    // SAFETY: query only; a closed handle returns zero.
    unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 }
}

fn editor_order(frame: HWND) -> Vec<usize> {
    let main = crate::park::own_handle(crate::screen::WINDOW_TITLE).unwrap() as HWND;
    let mut result = Vec::new();
    // SAFETY: only enumerate; keep the relative order of the two persistent windows.
    unsafe {
        let mut at = GetTopWindow(std::ptr::null_mut());
        while !at.is_null() {
            if at == frame || at == main {
                result.push(at as usize);
            }
            at = GetWindow(at, GW_HWNDNEXT);
        }
    }
    result
}

fn stage(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if n == 0 {
        super::preset_shots::stage(0, studio, window, cx);
        if std::env::var_os("XIRIUS_PRESET_FRONT_PIN").is_some() {
            super::preset_shots::native_command(studio, 4);
            let id = crate::plugin_reload_shots::id(&studio.borrow());
            crate::shot::run_for(&mut studio.borrow_mut(), 10.0, |s| {
                s.mixer.editor_pins.contains(&id)
            });
            assert!(studio.borrow().mixer.editor_pins.contains(&id));
        }
        if std::env::var_os("XIRIUS_PRESET_FRONT_TOPMOST").is_some() {
            let frame = crate::plugin_keys_shots::frame(&studio.borrow());
            // SAFETY: only this scenario's frame; no activation or desktop input.
            unsafe {
                SetWindowPos(
                    frame,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                );
            }
            assert!(topmost(frame));
        }
    } else if n == 1 {
        let frame = crate::plugin_keys_shots::frame(&studio.borrow());
        let activate = std::env::var_os("XIRIUS_PRESET_FRONT_ACTIVATE").is_some();
        if activate {
            foreground(frame);
        }
        let before = json!({"foreground": unsafe { GetForegroundWindow() == frame }, "topmost": topmost(frame), "owner": unsafe { GetWindow(frame, GW_OWNER) } as usize, "order": editor_order(frame)});
        std::fs::write(
            output(),
            serde_json::to_vec_pretty(&json!({"before": before})).unwrap(),
        )
        .unwrap();
        let start = Instant::now();
        TIMES.with_borrow_mut(|t| *t = Some((start, serde_json::Map::new())));
        let mut result = 0;
        // SAFETY: the preset control is in this scenario's editor frame. No OS input.
        assert_ne!(
            unsafe {
                SendMessageTimeoutW(
                    GetDlgItem(frame, 2),
                    BM_CLICK,
                    0,
                    0,
                    SMTO_ABORTIFHUNG,
                    5_000,
                    &mut result,
                )
            },
            0
        );
        mark("click_return_ms");
        observe(studio.clone(), start, window);
    } else if n == 2 {
        // Send to this GPUI surface, never to the foreground window or OS keyboard.
        let h = handle(studio, cx).expect("preset surface opened");
        h.update(cx, |view, window, cx| {
            gpui::EntityInputHandler::replace_text_in_range(view, None, "Clean", window, cx);
        })
        .unwrap();
        gpui::AnyWindowHandle::from(h)
            .update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke::parse("enter").unwrap(),
                        is_held: false,
                        prefer_character_input: false,
                    }),
                    cx,
                );
            })
            .unwrap();
    } else {
        super::preset_shots::stage(4, studio, window, cx);
        let frame = crate::plugin_keys_shots::frame(&studio.borrow());
        let id = crate::plugin_reload_shots::id(&studio.borrow());
        let mut data: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output()).unwrap()).unwrap();
        data["timing_ms"] = TIMES.with_borrow(|t| json!(t.as_ref().unwrap().1));
        data["after"] = json!({"foreground": unsafe { GetForegroundWindow() == frame }, "topmost": topmost(frame), "owner": unsafe { GetWindow(frame, GW_OWNER) } as usize, "order": editor_order(frame),
            "popup_closed": handle(studio, cx).is_none(), "selected": "Clean",
            "pinned": studio.borrow().mixer.editor_pins.contains(&id)});
        std::fs::write(output(), serde_json::to_vec_pretty(&data).unwrap()).unwrap();
        xirius_vst3::capture_window(frame as isize, &output().with_file_name("selected.bmp"))
            .unwrap();
        studio.borrow_mut().close_editor(id);
    }
}

fn observe(studio: Rc<RefCell<Studio>>, start: Instant, window: &mut Window) {
    window.on_next_frame(move |window, cx| {
        assert!(start.elapsed() < Duration::from_secs(10), "popup deadline");
        let Some(h) = handle(&studio, cx) else {
            observe(studio, start, window);
            return;
        };
        let frame = crate::plugin_keys_shots::frame(&studio.borrow());
        let hwnd = h.update(cx, |_, popup, _| {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let RawWindowHandle::Win32(raw) = HasWindowHandle::window_handle(popup).unwrap().as_raw() else { unreachable!() };
            raw.hwnd.get()
        }).unwrap();
        {
            let hwnd = hwnd as HWND;
            let mut button = RECT::default();
            let mut origin = POINT::default();
            // SAFETY: query handles belonging to the scenario and its plugin host.
            let (above, hit, owner) = unsafe {
                GetWindowRect(GetDlgItem(frame, 2), &mut button);
                ClientToScreen(hwnd, &mut origin);
                let mut at = GetTopWindow(std::ptr::null_mut());
                let mut above = false;
                while !at.is_null() {
                    if at == hwnd { above = true; break; }
                    if at == frame { break; }
                    at = GetWindow(at, GW_HWNDNEXT);
                }
                let hit = GetAncestor(WindowFromPoint(POINT { x: button.left + 24, y: button.bottom + 24 }), GA_ROOT);
                (above, hit == hwnd, GetWindow(hwnd, GW_OWNER) as usize)
            };
            let mut data: serde_json::Value = serde_json::from_slice(&std::fs::read(output()).unwrap()).unwrap();
            data["open"] = json!({"elapsed_ms": start.elapsed().as_secs_f64() * 1000.0,
                "above_editor": above, "point_is_popup": hit, "topmost": topmost(hwnd), "owner": owner,
                "popup_origin": [origin.x, origin.y], "button_bottom": [button.left, button.bottom]});
            std::fs::write(output(), serde_json::to_vec_pretty(&data).unwrap()).unwrap();
            assert!(!crate::park::is_foreground(hwnd as isize));
            xirius_vst3::capture_window(hwnd as isize, &output().with_file_name("popup.bmp"))
                .unwrap();
        }
    });
}
