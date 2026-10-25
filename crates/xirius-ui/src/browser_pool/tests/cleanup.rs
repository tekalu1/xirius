//! 「使われていない録音を整理」 (BRW-004): the dialog that lists what goes to the recycle bin.

use super::*;
use crate::cleanup_dialog::{Act, CleanLook};
use crate::popup::{Action, Look, PopupKind};

/// The press on 「…」 and on the row that opens the dialog.
fn open(t: &mut Song) {
    more(t);
    assert!(t.s.context_choose(&t.g, "使われていない録音を整理…"));
    assert!(!t.s.context_open(), "the menu gives way to the dialog");
    let p = t.s.popup.as_ref().expect("the dialog");
    assert_eq!(p.kind, PopupKind::Cleanup);
}

/// The dialog's parts as text: the title, each file and its size, the count and the total,
/// then the buttons.
fn parts(t: &Song) -> Vec<String> {
    let p = t.s.popup.as_ref().unwrap();
    p.layout(t.g.size)
        .cells
        .iter()
        .map(|c| match c.look {
            Look::Clean(CleanLook::File) => format!("{} {}", c.text, c.hint),
            Look::Clean(CleanLook::Total) => format!("= {} {}", c.text, c.hint),
            Look::Primary => format!("[{}]", c.text),
            _ => c.text.clone(),
        })
        .collect()
}

/// The press on the button 「`label`」, where it is drawn.
fn button(t: &mut Song, label: &str) {
    let l = t.s.popup.as_ref().unwrap().layout(t.g.size);
    let i = l.cells.iter().position(|c| c.text == label).unwrap();
    let (x, y) = t.s.center_of(&t.g, Hit::Item(i)).unwrap();
    let hit = t.s.hit(&t.g, x, y);
    assert_eq!(hit, Hit::Item(i));
    assert!(t.s.press(&t.g, hit));
}

fn size_of(t: &Song, file: &str) -> String {
    xirius_session::cleanup::megabytes(std::fs::metadata(t.dir().join(file)).unwrap().len())
}

#[test]
fn the_dialog_lists_the_unused_takes_with_their_sizes_and_puts_them_in_the_bin() {
    let mut t = song("cleanup");
    open(&mut t);
    let size = size_of(&t, VOCAL_2);
    assert!(size.ends_with(" MB"), "{size}");
    assert_eq!(
        parts(&t),
        [
            "使われていない録音を整理".to_string(),
            format!("Vocal_02.wav {size}"),
            format!("= 1 個 {size}"),
            "キャンセル".into(),
            "[ごみ箱に移動]".into(),
        ]
    );
    // In the middle of the window, the buttons at its right edge.
    let l = t.s.popup.as_ref().unwrap().layout(t.g.size);
    assert_eq!(l.rect.w, crate::cleanup_dialog::W);
    assert!((l.rect.x + l.rect.w / 2.0 - t.g.size.0 / 2.0).abs() < 1.0);
    assert!((l.rect.y + l.rect.h / 2.0 - t.g.size.1 / 2.0).abs() < 1.0);
    let trash = l.cells.last().unwrap().rect;
    assert!((l.rect.right() - trash.right() - 16.0).abs() < 0.5);
    let file = &l.cells[1];
    assert_eq!(file.icon, Some("audio"));
    assert_eq!(file.rect.h, 26.0);
    assert_eq!(trash.h, 28.0);
    // Nothing has gone yet.
    assert!(t.dir().join(VOCAL_2).is_file());
    button(&mut t, "ごみ箱に移動");
    assert!(t.s.popup.is_none());
    assert_eq!(t.s.error, None);
    assert!(t.bin().join("Vocal_02.wav").is_file());
    assert!(t.dir().join(VOCAL_1).is_file(), "a used take stays");
    assert_eq!(
        t.s.notice_text(),
        Some("使われていない録音 1 個をごみ箱に移動しました")
    );
    t.s.follow_browser();
    t.s.settle_pool();
    assert!(!rows(&t.s, &t.g).iter().any(|r| r.starts_with("Vocal_02")));
    more(&mut t);
    let row = row_of(&t.s, "使われていない録音を整理…");
    assert!(!row.enabled);
    assert_eq!(row.reason, "使われていない録音はありません");
}

#[test]
fn cancel_esc_and_a_press_outside_close_it_and_keep_the_files() {
    let mut t = song("cleanup-cancel");
    open(&mut t);
    button(&mut t, "キャンセル");
    assert!(t.s.popup.is_none());
    open(&mut t);
    assert!(t.s.cleanup_key(&t.g, "escape"));
    assert!(t.s.popup.is_none());
    assert!(!t.s.cleanup_key(&t.g, "escape"), "no dialog, no key taken");
    open(&mut t);
    let (x, y) = (4.0, t.g.size.1 / 2.0);
    let hit = t.s.hit(&t.g, x, y);
    t.s.press(&t.g, hit);
    assert!(t.s.popup.is_none());
    assert!(t.dir().join(VOCAL_2).is_file());
    assert!(!t.bin().exists());
    assert_eq!(t.s.notice_text(), None);
}

#[test]
fn the_keys_start_on_the_bin_and_tab_goes_to_cancel_with_a_ring() {
    let mut t = song("cleanup-keys");
    open(&mut t);
    let view = |t: &Song| t.s.popup.as_ref().unwrap().cleanup.clone().unwrap();
    assert_eq!(view(&t).focus, Act::Trash);
    assert!(!view(&t).keyed, "no ring before the keys move");
    // Other keys stay with the dialog.
    assert!(t.s.cleanup_key(&t.g, "space"));
    assert!(t.s.cleanup_key(&t.g, "tab"));
    assert_eq!(view(&t).focus, Act::Cancel);
    assert!(view(&t).keyed);
    assert!(t.s.cleanup_key(&t.g, "right"));
    assert_eq!(view(&t).focus, Act::Trash);
    assert!(t.s.cleanup_key(&t.g, "left"));
    assert!(t.s.cleanup_key(&t.g, "enter"));
    assert!(t.s.popup.is_none());
    assert!(
        t.dir().join(VOCAL_2).is_file(),
        "Enter on キャンセル keeps it"
    );
    // Enter on the first focus puts it in the bin.
    open(&mut t);
    assert!(t.s.cleanup_key(&t.g, "enter"));
    assert!(t.bin().join("Vocal_02.wav").is_file());
}

#[test]
fn a_long_list_shows_five_and_the_wheel_moves_on_while_the_total_counts_all() {
    let mut t = song("cleanup-many");
    let take = t.dir().join(VOCAL_2);
    for i in 3..=8 {
        let to = t.dir().join(format!("Media/Recordings/Vocal_0{i}.wav"));
        std::fs::copy(&take, to).unwrap();
    }
    open(&mut t);
    let files = |t: &Song| -> Vec<String> {
        parts(t)
            .into_iter()
            .filter(|p| p.starts_with("Vocal_"))
            .map(|p| p.split(' ').next().unwrap().to_string())
            .collect()
    };
    assert_eq!(files(&t).len(), 5);
    assert_eq!(files(&t)[0], "Vocal_02.wav");
    let one = std::fs::metadata(&take).unwrap().len();
    let total = xirius_session::cleanup::megabytes(one * 7);
    assert!(parts(&t).contains(&format!("= 7 個 {total}")));
    let r = t.s.popup.as_ref().unwrap().layout(t.g.size).rect;
    let (x, y) = (r.x + r.w / 2.0, r.y + 60.0);
    assert!(t.s.window_wheel(&t.g, (x, y), 0.0, -1.0, Mods::NONE, false));
    assert!(t.s.window_wheel(&t.g, (x, y), 0.0, -1.0, Mods::NONE, false));
    assert!(t.s.window_wheel(&t.g, (x, y), 0.0, -1.0, Mods::NONE, false));
    assert_eq!(
        files(&t),
        [
            "Vocal_04.wav",
            "Vocal_05.wav",
            "Vocal_06.wav",
            "Vocal_07.wav",
            "Vocal_08.wav"
        ]
    );
    button(&mut t, "ごみ箱に移動");
    assert_eq!(
        t.s.notice_text(),
        Some("使われていない録音 7 個をごみ箱に移動しました")
    );
}

#[test]
fn the_command_opens_it_and_says_why_when_there_is_nothing() {
    let mut t = song("cleanup-command");
    assert!(t.s.run_binding(&t.g, "view.cleanup"));
    assert_eq!(t.s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Cleanup));
    let l = t.s.popup.as_ref().unwrap().layout(t.g.size);
    assert!(
        l.cells
            .iter()
            .any(|c| c.action == Some(Action::Cleanup(Act::Trash)))
    );
    assert!(t.s.cleanup_key(&t.g, "enter"));
    assert!(!t.s.run_binding(&t.g, "view.cleanup"));
    assert!(t.s.popup.is_none());
    assert_eq!(
        t.s.error.take().as_deref(),
        Some("使われていない録音はありません")
    );
    // Over the session (MCP, the CLI) the window opens it on its next frame.
    std::fs::copy(
        t.dir().join(VOCAL_1),
        t.dir().join("Media/Recordings/Vocal_09.wav"),
    )
    .unwrap();
    t.s.song.run("view.cleanup", json!({})).unwrap();
    assert!(t.s.run_asked(&t.g));
    assert_eq!(parts(&t)[1].split(' ').next(), Some("Vocal_09.wav"));
}
