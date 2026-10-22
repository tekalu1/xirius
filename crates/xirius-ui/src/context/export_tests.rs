//! The menu of an export in the export panel (S44), pressed where the window presses it.
//! Called directly: no OS input, and the OS's file window is never opened.

use std::time::{Duration, Instant};

use super::tests::{choose, enabled, labels};
use crate::export::{Card, State};
use crate::popup::PopupKind;
use crate::studio::{Geometry, Rect, Studio};

const ANCHOR: Rect = Rect {
    x: 1500.0,
    y: -40.0,
    w: 28.0,
    h: 28.0,
};

fn until(s: &mut Studio, mut done: impl FnMut(&Studio) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        s.tick();
        if done(s) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    false
}

fn cards(s: &Studio) -> Vec<Card> {
    s.popup
        .as_ref()
        .and_then(|p| p.export.as_ref())
        .map(|v| v.cards.clone())
        .expect("the panel is open")
}

/// The recycle bin of these tests: `bin` beside the song, so a file an export replaces stays
/// in the test's own folder.
fn bin_in(dir: &std::path::Path) -> xirius_session::cleanup::Trash {
    let bin = dir.join("bin");
    std::sync::Arc::new(move |p: &std::path::Path| {
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        let mut to = bin.join(p.file_name().unwrap());
        let mut n = 1;
        while to.exists() {
            n += 1;
            to = bin.join(format!("{n}-{}", p.file_name().unwrap().to_string_lossy()));
        }
        std::fs::rename(p, to).map_err(|e| e.to_string())
    })
}

/// A song in a folder with one finished mixdown, and the panel open. The folder goes when the
/// guard is dropped (also when the test fails).
fn exported(tag: &str) -> (Studio, Geometry, std::path::PathBuf, tempfile::TempDir) {
    let dir_tmp = tempfile::Builder::new()
        .prefix(&format!("xirius-ctx-export-{tag}-"))
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("song");
    let mut s = Studio::new(crate::song::Song::sample_with_trash(bin_in(dir_tmp.path())));
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    let g = s.geometry(1600.0, 900.0);
    s.open_export(ANCHOR);
    assert!(s.export_action(crate::export::Act::Go));
    assert!(until(&mut s, |s| cards(s)[0].state == State::Done));
    (s, g, dir, dir_tmp)
}

/// The middle of the first card.
fn card_at(s: &Studio, g: &Geometry) -> (f32, f32) {
    let cell = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .into_iter()
        .find(|c| {
            matches!(
                c.look,
                crate::popup::Look::Xp(crate::export::XpLook::Card(_))
            )
        })
        .expect("a card");
    // Left of the buttons.
    (cell.rect.x + 60.0, cell.rect.y + cell.rect.h / 2.0)
}

fn menu(s: &mut Studio, g: &Geometry) {
    let (x, y) = card_at(s, g);
    assert!(s.scene_context(g, x, y));
    assert!(s.context_open());
}

fn panel_open(s: &Studio) -> bool {
    s.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Export)
}

#[test]
fn a_finished_export_has_the_rows_of_the_matrix_and_the_old_ones_can_be_written_again() {
    let (mut s, g, dir, _tmp) = exported("rows");
    menu(&mut s, &g);
    assert_eq!(
        labels(&s),
        [
            "聴く",
            "書き直す",
            "同じ設定で書き出しを開く",
            "曲に取り込む",
            "エクスプローラーで表示",
            "ごみ箱へ"
        ]
    );
    assert!(enabled(&s, "聴く"));
    assert!(enabled(&s, "エクスプローラーで表示"));
    assert!(!enabled(&s, "書き直す"), "the song has not changed");
    assert!(!choose(&mut s, &g, "書き直す"));
    assert!(s.context_open(), "a dim row leaves the menu");

    // The song changes: the card is out of date and can be written again.
    s.popup = None;
    s.ctx = None;
    let t = *s.project().tracks.keys().next().unwrap();
    s.command(|song| song.toggle_mute(t));
    s.open_export(ANCHOR);
    assert_eq!(cards(&s)[0].stale, Some(true));
    menu(&mut s, &g);
    assert!(enabled(&s, "書き直す"));
    let steps = s.song.steps();
    assert!(choose(&mut s, &g, "書き直す"));
    assert!(panel_open(&s), "the panel comes back where it was");
    assert_eq!(
        s.song.steps(),
        steps,
        "writing files is not a step of the song"
    );
    assert!(until(&mut s, |s| {
        let c = cards(s);
        c[0].state == State::Done && c[0].stale == Some(false)
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn listen_and_show_take_the_file_the_card_wrote() {
    let (mut s, g, dir, _tmp) = exported("listen");
    let path = s.export.jobs[0]["result"]["path"]
        .as_str()
        .unwrap()
        .to_string();
    menu(&mut s, &g);
    assert!(choose(&mut s, &g, "聴く"));
    assert_eq!(s.browser.playing.as_deref(), Some(path.as_str()));
    assert!(panel_open(&s));
    menu(&mut s, &g);
    assert!(choose(&mut s, &g, "エクスプローラーで表示"));
    assert_eq!(s.revealed, vec![std::path::PathBuf::from(&path)]);
    assert!(panel_open(&s));

    // The file is gone: nothing to hear or show.
    std::fs::remove_file(&path).unwrap();
    menu(&mut s, &g);
    assert!(!enabled(&s, "聴く"));
    assert!(!enabled(&s, "エクスプローラーで表示"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_same_settings_open_the_panel_as_that_export_was_made() {
    let (mut s, g, dir, _tmp) = exported("same");
    s.export_action(crate::export::Act::Format(
        xirius_session::export::Format::Flac,
    ));
    s.export_action(crate::export::Act::Rate(44_100));
    s.export_action(crate::export::Act::Bits(16));
    s.export_action(crate::export::Act::Mode(true));
    menu(&mut s, &g);
    assert!(choose(&mut s, &g, "同じ設定で書き出しを開く"));
    assert!(panel_open(&s));
    let v = s.popup.as_ref().unwrap().export.clone().unwrap();
    assert!(!v.stems);
    assert_eq!(v.format, xirius_session::export::Format::Wav);
    assert_eq!((v.rate, v.bits), (48_000, 24));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_trash_asks_once_and_takes_the_card_away() {
    let (mut s, g, dir, _tmp) = exported("trash");
    // The files go first, so nothing reaches the real trash.
    let path = s.export.jobs[0]["result"]["path"]
        .as_str()
        .unwrap()
        .to_string();
    std::fs::remove_file(&path).unwrap();
    menu(&mut s, &g);
    assert!(choose(&mut s, &g, "ごみ箱へ"));
    assert!(s.context_open(), "the row opens the one question");
    assert_eq!(labels(&s), ["戻る", "ファイルをごみ箱へ"]);
    assert_eq!(
        s.export.jobs.len(),
        1,
        "nothing is deleted by the first press"
    );
    assert!(choose(&mut s, &g, "ファイルをごみ箱へ"));
    assert!(s.error.is_none(), "{:?}", s.error);
    assert!(panel_open(&s));
    assert!(cards(&s).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_cards_trash_button_asks_before_touching_a_written_file() {
    let (mut s, g, dir, _tmp) = exported("button-confirm");
    let job = s.export.jobs[0]["job"].as_str().unwrap().to_string();
    let path = s.export.jobs[0]["result"]["path"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(s.export_action(crate::export::Act::Delete(job)));
    assert!(s.context_open());
    assert_eq!(labels(&s), ["ファイルをごみ箱へ"]);
    assert!(std::path::Path::new(&path).exists());
    assert_eq!(s.export.jobs.len(), 1);
    std::fs::remove_file(&path).unwrap();
    assert!(choose(&mut s, &g, "ファイルをごみ箱へ"));
    assert!(panel_open(&s));
    assert!(cards(&s).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn off_a_card_the_panel_takes_the_press_without_a_menu() {
    let (mut s, g, dir, _tmp) = exported("blank");
    let r = s.popup.as_ref().unwrap().layout(g.size).rect;
    assert!(s.scene_context(&g, r.x + 4.0, r.y + 4.0));
    assert!(!s.context_open());
    assert!(panel_open(&s));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_rows_are_the_export_commands_with_a_job() {
    let (mut s, g, dir, _tmp) = exported("commands");
    let job = s.export.jobs[0]["job"].as_str().unwrap().to_string();
    let path = s.export.jobs[0]["result"]["path"]
        .as_str()
        .unwrap()
        .to_string();
    // Listen and show, as the rows do.
    assert!(s.run_binding(&g, &format!("export.preview --job {job}")));
    assert_eq!(s.browser.playing.as_deref(), Some(path.as_str()));
    assert!(panel_open(&s));
    assert!(s.run_binding(&g, &format!("export.reveal_os --job {job}")));
    assert_eq!(s.revealed, vec![std::path::PathBuf::from(&path)]);
    // The same settings, and the panel comes back where it was.
    s.export_action(crate::export::Act::Format(
        xirius_session::export::Format::Flac,
    ));
    assert!(s.run_binding(&g, &format!("export.open --job {job}")));
    assert_eq!(
        s.popup.as_ref().unwrap().export.as_ref().unwrap().format,
        xirius_session::export::Format::Wav
    );
    // A job nobody knows finds nothing.
    assert!(!s.run_binding(&g, "export.open --job nowhere"));
    // The trash asks once, as the card's button does: nothing goes by the first word.
    let steps = s.song.steps();
    assert!(s.run_binding(&g, &format!("export.delete --job {job}")));
    assert_eq!(s.song.steps(), steps);
    assert!(s.context_open(), "the one question");
    assert_eq!(labels(&s), ["ファイルをごみ箱へ"]);
    assert_eq!(s.export.jobs.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_rewrite_row_stays_in_its_place_whether_it_is_dim_or_not() {
    let (mut s, g, dir, _tmp) = exported("dim");
    menu(&mut s, &g);
    let fresh = labels(&s);
    assert!(!enabled(&s, "書き直す"));
    s.popup = None;
    s.ctx = None;
    let t = *s.project().tracks.keys().next().unwrap();
    s.command(|song| song.toggle_mute(t));
    s.open_export(ANCHOR);
    menu(&mut s, &g);
    assert!(enabled(&s, "書き直す"));
    assert_eq!(labels(&s), fresh);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_out_of_date_export_stays_out_of_date_when_the_song_is_opened_again() {
    let (mut s, g, dir, _tmp) = exported("reopen");
    let job = cards(&s)[0].job.clone();
    s.popup = None;
    let t = *s.project().tracks.keys().next().unwrap();
    s.command(|song| song.toggle_mute(t));
    s.command(|song| song.save());
    assert!(s.error.is_none(), "{:?}", s.error);

    // Another song, and back to this one.
    s.new_song();
    s.finish_file_request(crate::studio::FileRequest::Open, &dir);
    assert!(s.error.is_none(), "{:?}", s.error);
    s.popup = None;
    s.open_export(ANCHOR);
    let c = cards(&s);
    assert_eq!(c[0].job, job);
    assert_eq!(c[0].stale, Some(true), "the record knows what the song was");
    assert!(s.bar_view().export_stale);
    menu(&mut s, &g);
    assert!(enabled(&s, "書き直す"));
    assert!(choose(&mut s, &g, "書き直す"));
    assert!(until(&mut s, |s| {
        let c = cards(s);
        c[0].state == State::Done && c[0].stale == Some(false)
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_export_from_before_records_kept_the_song_can_still_be_written_again() {
    let (mut s, g, dir, _tmp) = exported("old");
    let job = cards(&s)[0].job.clone();
    s.popup = None;
    s.new_song();
    // As an export of an earlier version left it, newer than the one made: no fingerprints.
    let store = xirius_session::jobs::Store::of_song(&dir);
    let mut v = store.read(&job).unwrap();
    let m = v.as_object_mut().unwrap();
    m.remove("base");
    m.insert("job".into(), serde_json::json!("export-before"));
    let started = m["started_ms"].as_u64().unwrap();
    m.insert("started_ms".into(), serde_json::json!(started + 1));
    store.write(&v).unwrap();
    s.finish_file_request(crate::studio::FileRequest::Open, &dir);
    assert!(s.error.is_none(), "{:?}", s.error);
    s.popup = None;
    s.open_export(ANCHOR);
    assert_eq!(cards(&s)[0].job, "export-before");
    assert_eq!(cards(&s)[0].stale, None);
    menu(&mut s, &g);
    assert!(
        enabled(&s, "書き直す"),
        "whether it is out of date is not known"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_card_and_its_menu_bring_the_files_in_at_the_head_of_the_exported_range() {
    let (mut s, g, dir, _tmp) = exported("import");
    let head = s.export.jobs[0]["base"]["span"][0].as_i64().unwrap();
    s.playhead = head + 7680;
    for how in ["button", "menu"] {
        let before = s.project();
        if how == "menu" {
            menu(&mut s, &g);
        }
        assert!(choose(&mut s, &g, "曲に取り込む"), "{how}");
        assert!(s.error.is_none(), "{:?}", s.error);
        let now = s.project();
        let added: Vec<_> = now
            .tracks
            .values()
            .filter(|t| !before.tracks.contains_key(&t.id))
            .map(|t| t.clips[0].start)
            .collect();
        assert_eq!(added, vec![head], "{how}: not where the playhead is");
        assert!(s.undo(&g));
        assert_eq!(s.project().tracks.len(), before.tracks.len());
        s.ctx = None;
        if !panel_open(&s) {
            s.open_export(ANCHOR);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The middle of the row of a file of the first card, by the track it is the stem of.
fn file_at(s: &Studio, g: &Geometry, name: &str) -> (f32, f32) {
    let cell = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .into_iter()
        .find(|c| {
            matches!(&c.look, crate::popup::Look::Xp(crate::export::XpLook::File(k)) if k.name == name)
        })
        .unwrap_or_else(|| panic!("no row of {name}"));
    (cell.rect.x + 20.0, cell.rect.y + cell.rect.h / 2.0)
}

#[test]
fn a_stale_stem_is_written_again_alone_from_its_own_row() {
    use crate::export::Act as X;
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-ctx-export-file-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("song");
    let bin = dir_tmp.path().join("bin");
    let mut s = Studio::new(crate::song::Song::sample_with_trash(bin_in(dir_tmp.path())));
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    let g = s.geometry(1600.0, 900.0);
    s.open_export(ANCHOR);
    // Stems of the first two tracks.
    s.export_action(X::Mode(true));
    s.export_action(X::Group(crate::export::Group::Selected));
    let ids: Vec<_> = s
        .popup
        .as_ref()
        .and_then(|p| p.export.as_ref())
        .unwrap()
        .tracks
        .iter()
        .map(|t| t.id)
        .collect();
    for id in &ids[2..] {
        s.export_action(X::Pick(*id));
    }
    assert!(s.export_action(X::Go));
    assert!(until(&mut s, |s| cards(s)[0].state == State::Done));
    let card = cards(&s)[0].clone();
    assert_eq!(card.kids.len(), 2, "{:?}", card.kids);
    assert_eq!(card.stale, Some(false));
    let changed = s.project().tracks[&ids[0]].name.clone();
    let (one, two) = (&card.kids[0], &card.kids[1]);
    assert_eq!(one.name, changed);
    let kept = std::path::PathBuf::from(&two.path);
    let before = (
        std::fs::read(&kept).unwrap(),
        std::fs::metadata(&kept).unwrap().modified().unwrap(),
    );
    // The clock moves on, so a file written again reads as written later.
    std::thread::sleep(Duration::from_millis(30));

    // The first track changes: only its file is stale.
    s.popup = None;
    s.command(|song| {
        song.run(
            "track.volume.set",
            serde_json::json!({ "ids": [ids[0].to_string()], "db": -9.0 }),
        )
        .map(|_| ())
    });
    assert!(s.error.is_none(), "{:?}", s.error);
    s.open_export(ANCHOR);
    let card = cards(&s)[0].clone();
    assert_eq!(card.stale, Some(true));
    assert_eq!(card.old, 1);
    assert_eq!(card.kids[0].stale, Some(true));
    assert_eq!(card.kids[1].stale, Some(false));
    let update = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .into_iter()
        .find(|c| c.action == Some(crate::popup::Action::Xp(X::Update(card.job.clone()))))
        .expect("the 更新 button");
    assert_eq!(update.tip, "変わった 1 本を書き直す");

    // The menu of the file that did not change cannot write it again.
    let (x, y) = file_at(&s, &g, &card.kids[1].name);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        ["聴く", "書き直す", "曲に取り込む", "エクスプローラーで表示"]
    );
    assert!(enabled(&s, "聴く"));
    assert!(!enabled(&s, "書き直す"));
    s.ctx = None;
    s.open_export(ANCHOR);

    // The file that did: its own row writes it again, and nothing else.
    let (x, y) = file_at(&s, &g, &changed);
    assert!(s.scene_context(&g, x, y));
    assert!(enabled(&s, "書き直す"));
    assert!(choose(&mut s, &g, "書き直す"));
    assert!(s.error.is_none(), "{:?}", s.error);
    assert!(panel_open(&s), "the panel comes back where it was");
    assert!(until(&mut s, |s| {
        let c = cards(s);
        c.len() == 1 && c[0].state == State::Done && c[0].stale == Some(false)
    }));
    let card = cards(&s)[0].clone();
    assert!(card.kids.iter().all(|k| k.stale == Some(false)));
    assert_eq!(
        (
            std::fs::read(&kept).unwrap(),
            std::fs::metadata(&kept).unwrap().modified().unwrap(),
        ),
        before,
        "the file that had not changed is left as it was"
    );
    let binned: Vec<String> = std::fs::read_dir(&bin)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(binned, vec![card.kids[0].file.clone()], "only the old one");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_files_rows_listen_show_and_bring_in_that_file() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-ctx-export-rows-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("song");
    let mut s = Studio::new(crate::song::Song::sample_with_trash(bin_in(dir_tmp.path())));
    s.command(|song| song.save_as(&dir));
    let g = s.geometry(1600.0, 900.0);
    s.open_export(ANCHOR);
    s.export_action(crate::export::Act::Mode(true));
    // These rows need real stem files, but not a render of the entire sample song.
    s.export_action(crate::export::Act::Range(crate::export::Range::Selection));
    s.export.fixed_span = Some((0, i64::from(s.project().ppq)));
    s.export.fixed_song = s.song.dir();
    assert!(s.export_action(crate::export::Act::Go), "{:?}", s.error);
    assert!(
        until(&mut s, |s| cards(s)[0].state != State::Running),
        "{:?}",
        cards(&s)
    );
    assert_eq!(cards(&s)[0].state, State::Done, "{:?}", cards(&s));
    let kid = cards(&s)[0].kids[1].clone();
    let (x, y) = file_at(&s, &g, &kid.name);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "聴く"));
    assert_eq!(s.browser.playing.as_deref(), Some(kid.path.as_str()));
    assert!(panel_open(&s));
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "エクスプローラーで表示"));
    assert_eq!(s.revealed, vec![std::path::PathBuf::from(&kid.path)]);
    assert!(s.scene_context(&g, x, y));
    let before = s.project();
    assert!(choose(&mut s, &g, "曲に取り込む"));
    assert!(s.error.is_none(), "{:?}", s.error);
    let added: Vec<_> = s
        .project()
        .tracks
        .values()
        .filter(|t| !before.tracks.contains_key(&t.id))
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(added.len(), 1, "that file alone: {added:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
