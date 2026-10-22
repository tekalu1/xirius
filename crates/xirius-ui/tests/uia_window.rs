//! UI-026 / UI-080: the parked window's tree is read through UI Automation, the way a screen
//! reader walks it. The track list comes as a Tree of TreeItems named after the tracks, and
//! the audio settings' input rows come as RadioButtons with a selection state.
#![cfg(windows)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationSelectionItemPattern,
    IUIAutomationTogglePattern, UIA_SelectionItemPatternId, UIA_TogglePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::BOOL;

struct Uninit;
impl Drop for Uninit {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn uia() -> (IUIAutomation, Uninit) {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        let a: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).unwrap();
        (a, Uninit)
    }
}

/// The window of the child process (its title may differ by song; the PID is certain).
fn windows_of(pid: u32) -> Vec<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let (want, out) = unsafe { &mut *(lp.0 as *mut (u32, Vec<HWND>)) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == *want && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            out.push(hwnd);
        }
        BOOL(1)
    }
    let mut found = (pid, Vec::new());
    unsafe {
        let _ = EnumWindows(
            Some(each),
            LPARAM(&mut found as *mut (u32, Vec<HWND>) as isize),
        );
    }
    found.1
}

/// (name, control type, toggle state) of every element in the subtree, depth-first.
fn read(a: &IUIAutomation, root: &IUIAutomationElement) -> Vec<(String, i32, Option<i32>)> {
    let mut out = Vec::new();
    let walker = unsafe { a.ControlViewWalker().unwrap() };
    let mut stack = vec![root.clone()];
    while let Some(el) = stack.pop() {
        unsafe {
            let name = el.CurrentName().map(|b| b.to_string()).unwrap_or_default();
            let ty = el.CurrentControlType().map(|t| t.0).unwrap_or(-1);
            let toggle = el
                .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                .ok()
                .and_then(|p| p.CurrentToggleState().ok())
                .map(|t| t.0)
                .or_else(|| {
                    el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                        UIA_SelectionItemPatternId,
                    )
                    .ok()
                    .and_then(|p| p.CurrentIsSelected().ok())
                    .map(|b| if b.as_bool() { 1 } else { 0 })
                });
            out.push((name, ty, toggle));
            if let Ok(child) = walker.GetFirstChildElement(&el) {
                let mut sibs = vec![child];
                while let Ok(next) = walker.GetNextSiblingElement(sibs.last().unwrap()) {
                    sibs.push(next);
                }
                stack.extend(sibs);
            }
        }
    }
    out
}

fn launch(env: Option<(&str, &str)>) -> (Child, tempfile::TempDir) {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-uia-")
        .tempdir()
        .unwrap();
    // Everything the app writes stays in the temp folder (the user's real keep
    // folder would decide which song opens and would be written on close).
    let keep = tmp.path().join("keep");
    std::fs::create_dir_all(&keep).unwrap();
    // `open: "sample"`: the sample song, whose track list the tree is read from.
    std::fs::write(
        keep.join("recent.json"),
        r#"{"songs": [], "open": "sample"}"#,
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xirius-ui"));
    cmd.args(["--park", "--silent"])
        .env("XIRIUS_CONFIG_DIR", tmp.path().join("prefs"))
        .env("XIRIUS_CRASH_DIR", tmp.path().join("crashes"))
        .env("XIRIUS_KEEP_DIR", &keep)
        .env("XIRIUS_TRASH_DIR", tmp.path().join("trash"))
        .env("XIRIUS_PLUGIN_CACHE", tmp.path().join("plugins.json"))
        .env("XIRIUS_PEAK_CACHE", tmp.path().join("peaks"))
        .env("XIRIUS_TRACK_PRESETS", tmp.path().join("presets"));
    if let Some((k, v)) = env {
        cmd.env(k, v);
    }
    (cmd.spawn().unwrap(), tmp)
}

/// The window, then its UIA subtree once the nodes are in it.
fn tree_of(child: &Child, a: &IUIAutomation, want: &str) -> Vec<(String, i32, Option<i32>)> {
    let limit = Instant::now() + Duration::from_secs(60);
    loop {
        for hwnd in windows_of(child.id()) {
            if let Ok(root) = unsafe { a.ElementFromHandle(hwnd) } {
                let rows = read(a, &root);
                if rows.iter().any(|(n, _, _)| n.contains(want)) {
                    return rows;
                }
            }
        }
        assert!(Instant::now() < limit, "no UIA tree with {want:?} in 60 s");
        std::thread::sleep(Duration::from_millis(500));
    }
}

// UIA_ControlTypeIds
const TREE: i32 = 50023;
const TREE_ITEM: i32 = 50024;
const RADIO_BUTTON: i32 = 50013;

#[test]
fn the_track_list_is_a_tree_of_named_items_to_uia() {
    let (a, _uninit) = uia();
    let (mut child, _tmp) = launch(None);
    let rows = tree_of(&child, &a, "Keys");
    child.kill().unwrap();
    let names: Vec<&str> = rows
        .iter()
        .filter(|(_, ty, _)| *ty == TREE_ITEM)
        .map(|(n, _, _)| n.as_str())
        .collect();
    assert!(
        rows.iter().any(|(_, ty, _)| *ty == TREE),
        "a Tree: {rows:?}"
    );
    for want in ["Keys", "Drums", "Bass"] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
}

#[test]
fn the_input_rows_read_as_radios_with_selection_to_uia() {
    let (a, _uninit) = uia();
    let (mut child, _tmp) = launch(Some(("XIRIUS_OPEN_SECTION", "audio")));
    let rows = tree_of(&child, &a, "In 1");
    child.kill().unwrap();
    let radios: Vec<_> = rows
        .iter()
        .filter(|(n, ty, _)| *ty == RADIO_BUTTON && n.starts_with("In "))
        .collect();
    assert_eq!(radios.len(), 2, "the fixture's two inputs: {rows:?}");
    assert!(
        radios
            .iter()
            .any(|(n, _, t)| n.as_str() == "In 1" && *t == Some(1)),
        "the selected input reads as on: {radios:?}"
    );
    assert!(
        radios
            .iter()
            .any(|(n, _, t)| n.as_str() == "In 2" && *t == Some(0)),
        "the other input reads as off: {radios:?}"
    );
}

/// The MIDI page has no audio inputs; the window used to have no UIA children at all while it
/// was open (the real window; the Audio test above runs on a fixture with two inputs).
#[test]
fn the_midi_page_has_uia_children_with_state() {
    let (a, _uninit) = uia();
    let (mut child, _tmp) = launch(Some(("XIRIUS_OPEN_SECTION", "midi")));
    let rows = tree_of(&child, &a, "PC のキーボード");
    child.kill().unwrap();
    assert!(
        rows.iter()
            .any(|(n, _, t)| n == "PC のキーボード" && t.is_some()),
        "the PC keyboard switch with a state: {rows:?}"
    );
    assert!(
        rows.iter().any(|(n, _, _)| n == "MIDI"),
        "the section tabs: {rows:?}"
    );
}
