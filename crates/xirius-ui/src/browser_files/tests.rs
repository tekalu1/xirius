//! The file browser on folders made for each test (BRW-002, BRW-005, BRW-006, BRW-008,
//! BRW-012, BRW-015).

use std::path::PathBuf;

use xirius_design::Layout;

use super::*;
use crate::browser::{BRow, Browser, BrowserHit, BrowserLayout, RailTab};
use crate::studio::Rect;

/// A folder of the test's own, removed when dropped (also when the test fails).
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-files-{name}-"))
        .tempdir()
        .unwrap()
}

/// A file of what the browser keeps, as a first run without a home folder writes it: a
/// missing file would start from the user's real Downloads and Music.
fn fresh(root: &Path) -> PathBuf {
    let file = root.join("browser.json");
    places::save(&file, &places::Prefs::first_run(None)).unwrap();
    file
}

/// A browser on the place `top` (its second tab), keeping nothing.
fn on(top: &Path) -> Browser {
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    b.add_place(top).unwrap();
    b
}

fn layout(b: &Browser) -> BrowserLayout {
    b.layout(Rect::new(52.0, 6.0, Layout::BROWSER_W, 470.0))
}

fn names(b: &Browser) -> Vec<String> {
    b.rows()
        .iter()
        .map(|r| match r {
            BRow::Dir { name, depth, .. } => format!("{}{name}/", "  ".repeat(*depth)),
            BRow::File { name, depth, .. } => format!("{}{name}", "  ".repeat(*depth)),
            BRow::Audition { .. } => "<audition>".into(),
            BRow::Note(n) => (*n).into(),
            _ => "?".into(),
        })
        .collect()
}

fn key(b: &Browser, name: &str) -> String {
    b.rows()
        .into_iter()
        .find_map(|r| match r {
            BRow::Dir { key, name: n, .. } | BRow::File { key, name: n, .. } if n == name => {
                Some(key)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no row {name}"))
}

#[test]
fn a_real_folder_lists_its_folders_first_and_only_sound_and_midi_files() {
    let top_tmp = folder("list");
    let top = top_tmp.path().join("素材");
    crate::import_shots::make_place(&top);
    let b = on(&top);
    assert_eq!(b.location().name, "素材");
    // notes.txt and .cache are left out.
    assert_eq!(
        names(&b),
        ["Drums/", "Loops/", "chords.mid", "idea.mid", "pad_44k.wav"]
    );
}

#[test]
fn the_tree_opens_a_folder_under_itself_and_the_crumbs_go_back() {
    let top_tmp = folder("tree");
    let top = top_tmp.path().join("素材");
    crate::import_shots::make_place(&top);
    let mut b = on(&top);
    let l = layout(&b);
    b.press(&l, BrowserHit::Dir(0));
    assert_eq!(
        names(&b)[..6],
        [
            "Drums/",
            "  hat_closed.wav",
            "  kick_01.wav",
            "  kick_02.wav",
            "  snare_01.wav",
            "Loops/"
        ]
    );
    let l = layout(&b);
    b.press(&l, BrowserHit::Dir(0));
    assert_eq!(names(&b)[1], "Loops/");

    b.enter_dir(&key(&b, "Loops"));
    assert_eq!(b.path, ["Loops"]);
    let l = layout(&b);
    assert_eq!(
        l.crumbs.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["素材", "Loops"]
    );
    assert_eq!(names(&b), ["loop_120bpm.wav", "loop_96bpm.wav"]);
    b.press(&l, BrowserHit::Crumb(0));
    assert!(b.path.is_empty());
    assert_eq!(names(&b)[0], "Drums/");
}

#[test]
fn the_search_finds_files_inside_closed_folders_of_the_place_only() {
    let root_tmp = folder("search");
    let root = root_tmp.path().to_path_buf();
    let top = root.join("素材");
    crate::import_shots::make_place(&top);
    // Outside the place: not found.
    std::fs::write(root.join("kick_outside.wav"), b"").unwrap();
    let mut b = on(&top);
    b.search = "KICK".into();
    assert_eq!(names(&b), ["kick_01.wav", "kick_02.wav"]);
    b.search = "zzz".into();
    assert_eq!(names(&b), ["見つかりません"]);
}

#[test]
fn rows_show_how_long_each_file_is_once_read() {
    let top_tmp = folder("length");
    let top = top_tmp.path().join("素材");
    crate::import_shots::make_place(&top);
    let b = on(&top);
    let _ = b.rows();
    settle(Duration::from_secs(10));
    let lens: Vec<(String, String)> = b
        .rows()
        .into_iter()
        .filter_map(|r| match r {
            BRow::File { name, len, .. } => Some((name, len)),
            _ => None,
        })
        .collect();
    assert_eq!(
        lens,
        [
            ("chords.mid".to_string(), "4 小節".to_string()),
            ("idea.mid".to_string(), "2 小節".to_string()),
            ("pad_44k.wav".to_string(), "0:06".to_string()),
        ]
    );
    assert_eq!(clock(83.4), "1:23");
    assert_eq!(clock(0.2), "0.2 秒");
    assert_eq!(clock(0.0), "0.1 秒");
    assert_eq!(clock(0.96), "0:01");
    assert_eq!(clock(3725.0), "1:02:05");
}

#[test]
fn the_song_tab_waits_for_a_folder_and_hides_what_the_song_keeps() {
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    assert_eq!(names(&b), [NO_FOLDER]);
    let song_tmp = folder("song");
    let song = song_tmp.path().to_path_buf();
    for own in SONG_OWN {
        std::fs::create_dir_all(song.join(own)).unwrap();
    }
    b.set_song_dir(Some(song.clone()));
    assert_eq!(names(&b), [EMPTY]);
    crate::import_shots::make_place(&song.join("Audio"));
    forget();
    assert_eq!(names(&b), ["Audio/"]);
    // Deeper down the same names are shown.
    b.enter_dir(&key(&b, "Audio"));
    std::fs::create_dir_all(song.join("Audio/history")).unwrap();
    forget();
    assert_eq!(names(&b)[0], "Drums/");
    assert!(names(&b).contains(&"history/".to_string()));

    let _ = std::fs::remove_dir_all(&song);
    forget();
    assert_eq!(names(&b), [GONE]);
}

#[test]
fn a_place_is_added_shown_kept_and_taken_away_but_never_the_song() {
    let root_tmp = folder("places");
    let root = root_tmp.path().to_path_buf();
    let file = fresh(&root);
    let (a, c) = (root.join("Samples"), root.join("Loops"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&c).unwrap();
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    b.use_prefs(file.clone()).unwrap();
    assert_eq!(b.add_place(&a), Ok("Samples".into()));
    assert_eq!(b.add_place(&c), Ok("Loops".into()));
    assert_eq!(b.location().name, "Loops");
    assert!(b.add_place(&a).is_err(), "the same folder twice");

    // Another browser (after a restart) has the tabs.
    let mut again = Browser::new();
    again.use_prefs(file.clone()).unwrap();
    let tabs: Vec<String> = again.locations().into_iter().map(|l| l.name).collect();
    assert_eq!(tabs, [places::SONG, "Samples", "Loops"]);

    // The cross sits on the shown tab only, and never on the song's.
    let l = layout(&b);
    assert!(l.remove.is_some());
    let (x, y) = b.center_of(&l, BrowserHit::LocRemove(2)).unwrap();
    assert_eq!(b.hit(&l, x, y), Some(BrowserHit::LocRemove(2)));
    b.press(&l, BrowserHit::LocRemove(2));
    assert_eq!(b.location().name, "Samples");
    assert!(c.is_dir(), "the folder itself stays");
    b.set_location(0);
    assert_eq!(layout(&b).remove, None);
    assert!(b.remove_place(0).is_err());

    let mut again = Browser::new();
    again.use_prefs(file).unwrap();
    assert_eq!(again.locations().len(), 2);
}

#[test]
fn the_plus_asks_for_a_folder() {
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    let l = layout(&b);
    let (x, y) = b.center_of(&l, BrowserHit::LocAdd).unwrap();
    assert_eq!(b.hit(&l, x, y), Some(BrowserHit::LocAdd));
    assert_eq!(
        b.press(&l, BrowserHit::LocAdd),
        crate::browser::BrowserEffect::AddPlace
    );
}

#[test]
fn many_tabs_share_the_width_and_stay_inside_the_panel() {
    let root_tmp = folder("many");
    let root = root_tmp.path().to_path_buf();
    let mut b = Browser::new();
    b.press_rail(RailTab::Files);
    for i in 0..6 {
        let d = root.join(format!("とても長い名前のフォルダー {i}"));
        std::fs::create_dir_all(&d).unwrap();
        b.add_place(&d).unwrap();
    }
    let l = layout(&b);
    assert_eq!(l.tabs.len(), 7);
    let panel_right = 52.0 + Layout::BROWSER_W;
    let add = l.add.expect("the plus is there");
    assert!(add.x + add.w <= panel_right + 0.5);
    for r in &l.tabs {
        assert!(r.x + r.w <= add.x + 0.5, "{r:?} {add:?}");
    }
}

#[test]
fn favourites_and_hidden_are_kept_and_another_programs_change_is_read() {
    let root_tmp = folder("prefs");
    let root = root_tmp.path().to_path_buf();
    let file = fresh(&root);
    let mut b = Browser::new();
    b.use_prefs(file.clone()).unwrap();
    b.toggle_favourite("Dexed");
    b.set_hidden("Surge XT", true);
    let p = places::load(&file).unwrap();
    assert!(p.favourites.contains("Dexed"));
    assert!(p.hidden.contains("Surge XT"));

    // The CLI shows hidden things meanwhile: the screen follows on its next poll.
    std::thread::sleep(Duration::from_millis(20));
    let mut p = p;
    p.show_hidden = true;
    p.favourites.remove("Dexed");
    places::save(&file, &p).unwrap();
    assert!(b.reload_prefs());
    assert!(b.show_hidden);
    assert!(!b.favourites.contains("Dexed"));
    assert!(!b.reload_prefs(), "nothing new");
}

#[test]
fn a_file_that_cannot_be_read_says_so() {
    let root_tmp = folder("broken");
    let root = root_tmp.path().to_path_buf();
    let file = root.join("browser.json");
    std::fs::write(&file, "{ not json").unwrap();
    let mut b = Browser::new();
    assert!(b.use_prefs(file).is_err());
}

#[test]
fn choosing_an_audio_file_starts_its_audition_and_a_midi_file_does_not() {
    let top_tmp = folder("audition");
    let top = top_tmp.path().join("素材");
    crate::import_shots::make_place(&top);
    let mut b = on(&top);
    let pad = key(&b, "pad_44k.wav");
    let l = layout(&b);
    let i = l
        .rows
        .iter()
        .position(|p| matches!(&p.row, BRow::File { name, .. } if name == "pad_44k.wav"))
        .unwrap();
    b.press(&l, BrowserHit::File(i));
    assert_eq!(b.playing.as_deref(), Some(pad.as_str()));
    assert_eq!(names(&b)[i + 1], "<audition>");
    let l = layout(&b);
    let (x, y) = b.center_of(&l, BrowserHit::Audition).unwrap();
    assert_eq!(b.hit(&l, x, y), Some(BrowserHit::Audition));
    b.press(&l, BrowserHit::Audition);
    assert_eq!(b.playing, None);
    b.press(&l, BrowserHit::Audition);
    assert!(b.playing.is_some());

    // A MIDI file: chosen, no audition row, and what played stops.
    b.select_file(&key(&b, "idea.mid"));
    assert_eq!(b.playing, None);
    assert!(!names(&b).contains(&"<audition>".to_string()));
}
