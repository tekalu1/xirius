//! Parked playback and input dispatch with process-local focus checks.
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use gpui::{App, Window};
use std::{cell::RefCell, rc::Rc};

pub const QUIET: &[Shot] = &[
    Shot {
        name: "quiet-00-before",
        acts: &[Act::Stage(Play::Window(check), 0)],
    },
    Shot {
        name: "quiet-01-playing",
        acts: &[
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Window(check), 1),
        ],
    },
    Shot {
        name: "quiet-02-stopped",
        acts: &[
            Act::Press(Target::Top(BarButton::Stop)),
            Act::Stage(Play::Window(check), 2),
        ],
    },
];

fn check(n: u8, studio: &Rc<RefCell<Studio>>, _: &mut Window, _: &mut App) {
    let studio = studio.borrow();
    assert!(matches!(
        studio.transport.mode,
        xirius_session::output::OutputMode::Silent { .. }
    ));
    assert!(!studio.transport.observe_inputs);
    assert_eq!(studio.transport.playing, n == 1);
    #[cfg(windows)]
    audit_window();
    assert!(studio.error.is_none(), "{:?}", studio.error);
    eprintln!("quiet-check stage={n} ok=true");
}

#[cfg(windows)]
fn audit_window() {
    use std::ffi::c_void;
    type Hwnd = *mut c_void;
    #[repr(C)]
    struct GuiThreadInfo {
        size: u32,
        flags: u32,
        active: Hwnd,
        focus: Hwnd,
        capture: Hwnd,
        menu: Hwnd,
        move_size: Hwnd,
        caret: Hwnd,
        rect: [i32; 4],
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetGUIThreadInfo(thread: u32, info: *mut GuiThreadInfo) -> i32;
        fn GetWindowThreadProcessId(window: Hwnd, process: *mut u32) -> u32;
    }
    let hwnd = crate::park::own_handle(crate::screen::WINDOW_TITLE).unwrap();
    assert!(!crate::park::is_foreground(hwnd));
    let (x, y) = crate::park::origin(hwnd).unwrap();
    assert!(x <= crate::park::PARK_AT && y <= crate::park::PARK_AT);
    let mut info = GuiThreadInfo {
        size: std::mem::size_of::<GuiThreadInfo>() as u32,
        flags: 0,
        active: std::ptr::null_mut(),
        focus: std::ptr::null_mut(),
        capture: std::ptr::null_mut(),
        menu: std::ptr::null_mut(),
        move_size: std::ptr::null_mut(),
        caret: std::ptr::null_mut(),
        rect: [0; 4],
    };
    // SAFETY: the live window belongs to this process and info has the documented layout.
    unsafe {
        let thread = GetWindowThreadProcessId(hwnd as Hwnd, std::ptr::null_mut());
        assert_ne!(GetGUIThreadInfo(thread, &mut info), 0);
    }
    assert!(
        info.active.is_null() && info.focus.is_null(),
        "the parked window acquired OS focus"
    );
}
