//! The editor window's title bar takes the app's colours (UI-183): the colour given at open is
//! on the window before it is first shown, and a later change (dark to light) reaches it. The
//! window opens off every screen and never takes the foreground; the colours are read back from
//! the window manager (the colours can only be set, so what it took is the editor's own
//! record and the dark frame is asked of the system).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use xirius_vst3::{Caption, Module, Placement, Plugin};

fn fixture() -> Plugin {
    xirius_test_vst3::fixture_marker();
    let deps: PathBuf = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = deps.join("xirius_test_vst3.dll");
    assert!(dll.exists(), "the fixture's cdylib is built with the tests");
    let module = Arc::new(Module::load(&dll).expect("load"));
    let class = module.classes().into_iter().next().expect("a class");
    Plugin::new(module, &class).expect("create")
}

const DARK: Caption = Caption {
    dark: true,
    fill: 0x121318,
    text: 0xf4f6ff,
};
const LIGHT: Caption = Caption {
    dark: false,
    fill: 0xeceef4,
    text: 0x0f1330,
};

#[test]
fn the_caption_colour_is_on_the_window_from_the_start_and_follows_a_change() {
    let plugin = fixture();
    let mut editor = plugin
        .open_editor_at(
            "Xirius Test Latency",
            Placement {
                caption: Some(DARK),
                ..Placement::offscreen()
            },
        )
        .expect("open");
    editor.pump(Duration::from_millis(30));
    assert!(!editor.took_foreground());
    assert_eq!(editor.caption(), Some(DARK), "dark from the first frame");
    assert!(editor.dark_frame());

    editor.set_caption(LIGHT);
    assert_eq!(editor.caption(), Some(LIGHT), "light after the change");
    assert!(!editor.dark_frame());
    editor.set_caption(DARK);
    assert_eq!(editor.caption(), Some(DARK));
    assert!(editor.dark_frame());
}

#[test]
fn a_window_opened_without_a_caption_keeps_the_systems() {
    let plugin = fixture();
    let editor = plugin
        .open_editor_at("Xirius Test Latency", Placement::offscreen())
        .expect("open");
    assert_eq!(editor.caption(), None);
}
