use crate::{
    mixer::editors::Where,
    shot::{Act, Part, Play, Shot, Target},
    studio::Studio,
};
use serde_json::json;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    UI::WindowsAndMessaging::{
        EnumChildWindows, FindWindowExW, GetClassNameW, GetForegroundWindow,
        GetWindowThreadProcessId, SendMessageW, WM_CHAR, WM_CLOSE, WM_GETTEXT, WM_KEYDOWN,
        WM_KEYUP,
    },
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "key01-local-text",
        acts: &[
            Act::Reset,
            staged(0),
            Act::Press(Target::Strip(Part::Insert, "Vocal")),
            staged(2),
        ],
    },
    Shot {
        name: "key02-local-playing",
        acts: &[staged(3)],
    },
    Shot {
        name: "key03-local-stopped",
        acts: &[staged(4)],
    },
    Shot {
        name: "key04-host-text",
        acts: &[
            Act::Reset,
            staged(1),
            Act::Press(Target::Strip(Part::Insert, "Vocal")),
            staged(2),
        ],
    },
    Shot {
        name: "key05-host-playing",
        acts: &[staged(3)],
    },
    Shot {
        name: "key06-host-stopped",
        acts: &[staged(4)],
    },
];
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Studio(stage), n)
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// The text box of the fixture's editor, wherever it sits below the frame.
fn edit_of(frame: HWND) -> HWND {
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> i32 {
        let mut name = [0u16; 16];
        // SAFETY: the buffer is as long as stated; `found` points at the caller's slot.
        unsafe {
            let len = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32).max(0) as usize;
            if String::from_utf16_lossy(&name[..len]).eq_ignore_ascii_case("EDIT") {
                *(found as *mut HWND) = hwnd;
                return 0;
            }
        }
        1
    }
    let mut found: HWND = std::ptr::null_mut();
    // SAFETY: the callback only runs inside this call.
    unsafe { EnumChildWindows(frame, Some(visit), &mut found as *mut HWND as LPARAM) };
    found
}

fn frame(s: &Studio) -> HWND {
    let id = crate::plugin_reload_shots::id(s);
    if let Some(editor) = s.mixer.editors.iter().find(|e| e.id == id) {
        match &editor.at {
            Where::Local { editor, .. } | Where::Apart { editor, .. } => {
                return editor.hwnd() as HWND;
            }
            Where::Host { .. } => {}
        }
    }
    let Some(pid) = s
        .transport
        .host()
        .and_then(|h| h.plugins().server_of(id))
        .and_then(|h| h.pid())
    else {
        return std::ptr::null_mut();
    };
    // SAFETY: enumerate only the editor class, accepting a window solely from our host PID.
    unsafe {
        let class = wide("XiriusPluginEditor");
        let mut after = std::ptr::null_mut();
        loop {
            let hwnd = FindWindowExW(
                std::ptr::null_mut(),
                after,
                class.as_ptr(),
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return hwnd;
            }
            let mut actual = 0;
            GetWindowThreadProcessId(hwnd, &mut actual);
            if actual == pid {
                return hwnd;
            }
            after = hwnd;
        }
    }
}

fn text(s: &mut Studio) {
    // The fixture makes its edit after the frame; on a busy machine that can lag behind.
    crate::shot::run_for(s, 10.0, |s| {
        let f = frame(s);
        !f.is_null() && !edit_of(f).is_null()
    });
    let hwnd = frame(s);
    assert!(!hwnd.is_null(), "{:?}", s.error);
    assert!(!s.transport.playing);
    // SAFETY: the frame is an editor of this run; the edit is its fixture's direct child.
    unsafe {
        assert_ne!(GetForegroundWindow(), hwnd);
        let edit = edit_of(hwnd);
        assert!(!edit.is_null());
        for ch in "波形 Ab".encode_utf16() {
            SendMessageW(edit, WM_CHAR, ch as usize, 1);
        }
        let mut value = [0u16; 128];
        let len = SendMessageW(edit, WM_GETTEXT, value.len(), value.as_mut_ptr() as isize) as usize;
        assert_eq!(String::from_utf16_lossy(&value[..len]), "波形 Ab");
        // The Space that picks a conversion candidate reaches the frame as VK_PROCESSKEY.
        SendMessageW(hwnd, WM_KEYDOWN, 0xE5, 1);
    }
    s.idle();
    assert!(!s.transport.playing, "Space in text belongs to the plugin");
    let args: Vec<_> = std::env::args().collect();
    let dir = args
        .windows(2)
        .find(|v| v[0] == "--shot")
        .map(|v| std::path::PathBuf::from(&v[1]))
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../target/plugin-keys-{}", std::process::id()))
        });
    std::fs::create_dir_all(&dir).unwrap();
    let local = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .inserts[0]
        .isolation
        == xirius_core::model::PluginIsolation::InProcess;
    let path = dir.join(if local {
        "editor01-local.bmp"
    } else {
        "editor02-host.bmp"
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        for e in &mut s.mixer.editors {
            if let Where::Local { editor, .. } | Where::Apart { editor, .. } = &mut e.at {
                editor.pump_own(std::time::Duration::ZERO);
            }
        }
        let capture = xirius_vst3::capture_window(hwnd as isize, &path).unwrap();
        if capture.distinct_colors > 2 {
            break;
        }
        assert!(std::time::Instant::now() < until, "{capture:?}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn space(s: &mut Studio, playing: bool) {
    // While the mix is built again for a latency the plugin reported late, the window the
    // editor sits on moves to the instance that plays then (ENG-065): a press the leaving
    // window takes is gone with it, so a press that changed nothing is tried again on the
    // window it is on now.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while s.transport.playing != playing && std::time::Instant::now() < until {
        let hwnd = frame(s);
        if hwnd.is_null() {
            // Between the window that went and the one that opens there can be none.
            crate::shot::run_for(s, 0.2, |s| s.transport.playing == playing);
            continue;
        }
        // SAFETY: targeted messages to our editor frame; no OS keyboard or focus changes.
        unsafe {
            SendMessageW(hwnd, WM_KEYDOWN, 0x20, 1);
            SendMessageW(hwnd, WM_KEYUP, 0x20, 1);
        }
        // The host editor answers through its own process; on a busy machine that lags.
        crate::shot::run_for(s, 1.0, |s| s.transport.playing == playing);
    }
    assert_eq!(s.transport.playing, playing, "{:?}", s.error);
}
fn stage(s: &mut Studio, n: u8) {
    match n {
        0 | 1 => {
            crate::plugin_reload_shots::prepare(s, 0);
            if n == 0 {
                assert!(s.run_command("plugin.isolation.set", json!({"ids":[crate::plugin_reload_shots::id(s).to_string()], "mode":xirius_core::model::PluginIsolation::InProcess.name()})));
            }
        }
        2 => text(s),
        3 => {
            space(s, true);
        }
        4 => {
            space(s, false);
            // A plugin opened late may report a latency the mix did not assume; the mix built
            // again for it moves the window to the plugin's new slot, so close it once that is in.
            crate::shot::run_for(s, 0.5, |_| false);
            crate::shot::run_for(s, 10.0, |s| {
                !s.transport.host().is_some_and(|h| h.swap_pending())
            });
            // SAFETY: own editor's close action; it hides until the host drops it.
            unsafe {
                SendMessageW(frame(s), WM_CLOSE, 0, 0);
            }
            crate::shot::run_for(s, 7.0, |s| s.mixer.editors.is_empty());
            assert!(s.mixer.editors.is_empty());
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::MixerHit;
    #[test]
    fn native_text_stays_in_the_editor_and_frame_space_toggles_local_and_host_playback() {
        for mode in [0, 1] {
            let mut s = Studio::sample();
            stage(&mut s, mode);
            let g = s.geometry(1600.0, 900.0);
            let t = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Vocal")
                .unwrap()
                .id;
            let (x, y) = s
                .mixer_center(&g, MixerHit::Open(t, crate::plugin_reload_shots::id(&s)))
                .unwrap();
            assert!(s.chain_down(&g, x, y));
            assert!(s.fx_up(&g, x, y, false));
            for n in [2, 3, 4] {
                stage(&mut s, n);
            }
        }
    }
}
