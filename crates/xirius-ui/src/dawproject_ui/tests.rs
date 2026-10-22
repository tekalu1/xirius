//! DAWproject from the song menu, the file dialog's answer and a drop (EXP-006, EXP-007). The
//! files are written in a folder of the test's own, and a file written over goes to a bin in
//! it: nothing reaches the recycle bin of the PC.

use std::path::{Path, PathBuf};

use xirius_core::dawproject::zip::ZipWriter;
use xirius_core::model::TrackKind;

use super::*;
use crate::popup::Action;
use crate::song::Song;
use crate::studio::{FileRequest, Hit};
use crate::topbar::BarButton;

/// A folder of its own for one test's files.
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-dawproject-{name}-"))
        .tempdir()
        .unwrap()
}

/// Moves a file into `dir/bin` in place of the recycle bin.
fn bin_in(dir: &Path) -> xirius_session::cleanup::Trash {
    let bin = dir.join("bin");
    std::sync::Arc::new(move |p: &Path| {
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    })
}

fn studio_in(dir: &Path) -> (Studio, Geometry) {
    let s = Studio::new(Song::sample_with_trash(bin_in(dir)));
    let g = s.geometry(1440.0, 900.0);
    (s, g)
}

fn press_cell(s: &mut Studio, g: &Geometry, act: Action) -> bool {
    let i = s
        .popup
        .as_ref()
        .expect("a pop-over")
        .layout(g.size)
        .cells
        .iter()
        .position(|c| c.action.as_ref() == Some(&act))
        .unwrap_or_else(|| panic!("{act:?} in the pop-over"));
    s.press(g, Hit::Item(i))
}

fn middle(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// A `.dawproject` whose track has a VST2 plugin, a send and notes on an audio track, and
/// whose song has two scenes: four things a song here cannot take.
fn with_four_left_out(dir: &Path) -> PathBuf {
    let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Other" version="1"/>
  <Transport><Tempo unit="bpm" value="100" id="t0"/></Transport>
  <Structure>
    <Track contentType="audio" id="a" name="Vox">
      <Channel role="regular" id="ac" destination="mc">
        <Devices>
          <Vst2Plugin deviceID="1234" deviceName="Old Verb" id="d1"/>
        </Devices>
        <Sends>
          <Send destination="ac" id="s1"><Volume unit="linear" value="1" id="s1v"/></Send>
        </Sends>
      </Channel>
    </Track>
    <Track contentType="audio notes" id="m" name="Master">
      <Channel role="master" id="mc"/>
    </Track>
  </Structure>
  <Arrangement>
    <Lanes timeUnit="beats">
      <Lanes track="a">
        <Clips>
          <Clip time="0" duration="4"><Notes><Note time="0" duration="1" key="60"/></Notes></Clip>
        </Clips>
      </Lanes>
    </Lanes>
  </Arrangement>
  <Scenes><Scene id="sc1"/><Scene id="sc2"/></Scenes>
</Project>"##;
    let path = dir.join("Demo Song.dawproject");
    let mut zip = ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.add("project.xml", xml.as_bytes(), true).unwrap();
    zip.finish().unwrap();
    path
}

#[test]
fn the_song_menu_asks_where_to_write_and_what_to_open() {
    let dir = folder("menu");
    let (mut s, g) = studio_in(dir.path());
    assert!(s.press(&g, Hit::Bar(BarButton::SongName)));
    assert!(press_cell(&mut s, &g, Action::ExportDawproject));
    assert_eq!(s.file_request, Some(FileRequest::ExportDawproject));
    assert!(s.popup.is_none());
    s.file_request = None;
    assert!(s.press(&g, Hit::Bar(BarButton::SongName)));
    assert!(press_cell(&mut s, &g, Action::OpenDawproject));
    assert_eq!(s.file_request, Some(FileRequest::OpenDawproject));
    assert!(s.popup.is_none());
}

#[test]
fn writing_says_how_far_then_names_the_file_with_what_it_left_out_and_its_folder() {
    let dir = folder("export");
    let (mut s, g) = studio_in(dir.path());
    let expected = xirius_session::dawproject::skipped(&s.project(), None, true);
    // The dialog's answer without the ending: it is added.
    s.finish_file_request(FileRequest::ExportDawproject, &dir.path().join("夜の散歩"));
    let busy = s.toast.clone().expect("the toast while it writes");
    assert!(
        busy.busy && busy.text.starts_with(EXPORTING),
        "{}",
        busy.text
    );
    assert!(busy.text.ends_with('%'));
    assert!(crate::toast::layout(&s, &g).unwrap().close.is_none());
    s.dawproject_wait();
    assert!(s.dawproject.is_none());
    let out = dir.path().join("夜の散歩.dawproject");
    assert!(out.is_file());
    let t = s.toast.clone().expect("the toast when it is written");
    assert_eq!(t.text, "夜の散歩.dawproject を書き出しました");
    assert!(t.done && !t.busy && !t.alert);
    let parts = crate::toast::layout(&s, &g).unwrap();
    assert!(
        !expected.is_empty(),
        "the sample song has things a file cannot carry"
    );
    let (label, answer) = t.button.clone().unwrap();
    assert_eq!(label, format!("{LEFT_OUT} {}", expected.len()));
    assert_eq!(answer, Answer::Skipped(List::of(LEFT_OUT, &expected)));
    assert_eq!(t.also.clone().unwrap().0, REVEAL);

    // The count opens the list over the toast, which stays; the list's left edge is the toast's.
    let (x, y) = middle(parts.button.unwrap());
    assert!(s.toast_press(&g, x, y));
    assert!(s.toast.is_some());
    let p = s.popup.as_ref().expect("the list");
    assert_eq!(p.kind, PopupKind::Skipped);
    let l = p.layout(g.size);
    assert_eq!(l.rect.x, parts.rect.x);
    assert_eq!(l.rect.bottom(), parts.rect.y - crate::popup::ANCHOR_GAP);
    assert_eq!(l.cells[0].text, LEFT_OUT);
    let rows: Vec<(String, String)> = l.cells[1..]
        .iter()
        .map(|c| (c.text.clone(), c.hint.clone()))
        .collect();
    assert_eq!(rows, List::of(LEFT_OUT, &expected).rows);

    // The folder: shown in the window's own file window only in the real window.
    let (x, y) = middle(parts.also.unwrap());
    assert!(s.toast_press(&g, x, y));
    assert_eq!(s.revealed, vec![out.clone()]);
    assert!(s.toast.is_none());
    assert_eq!(s.error, None);
}

#[test]
fn writing_over_a_file_sends_the_old_one_to_the_bin() {
    let dir = folder("over");
    let (mut s, _g) = studio_in(dir.path());
    let out = dir.path().join("Song.dawproject");
    std::fs::write(&out, b"old").unwrap();
    assert!(s.dawproject_export(&out));
    s.dawproject_wait();
    assert_eq!(
        std::fs::read(dir.path().join("bin/Song.dawproject")).unwrap(),
        b"old"
    );
    assert_ne!(std::fs::read(&out).unwrap(), b"old");
    assert!(s.toast.as_ref().is_some_and(|t| t.done));
}

#[test]
fn a_written_song_opens_again_as_a_new_unsaved_song_with_its_tracks() {
    let dir = folder("round");
    let (mut s, _g) = studio_in(dir.path());
    let names = |p: &xirius_core::model::Project| {
        let mut n: Vec<String> = p
            .tracks
            .values()
            .filter(|t| t.kind != TrackKind::Master)
            .map(|t| t.name.clone())
            .collect();
        n.sort();
        n
    };
    let before = names(&s.project());
    let out = dir.path().join("Round.dawproject");
    assert!(s.dawproject_export(&out));
    s.dawproject_wait();

    let mut s = Studio::new(Song::empty());
    s.finish_file_request(FileRequest::OpenDawproject, &out);
    let busy = s.toast.clone().expect("the toast while it reads");
    assert!(busy.busy && busy.text.starts_with(READING), "{}", busy.text);
    s.dawproject_wait();
    assert_eq!(s.error, None);
    assert_eq!(names(&s.project()), before);
    assert_eq!(s.project().name, "Round");
    assert!(s.song().dir().is_none(), "not saved yet");
    assert!(!s.start_shown());
    // Everything came across: nothing to list.
    assert!(s.toast.as_ref().is_none_or(|t| !t.alert), "{:?}", s.toast);
}

#[test]
fn what_could_not_be_read_is_a_count_on_the_toast_and_a_list_behind_see() {
    let dir = folder("left");
    let path = with_four_left_out(dir.path());
    let mut s = Studio::new(Song::empty());
    let g = s.geometry(1440.0, 900.0);
    assert!(s.dawproject_open(&path));
    s.dawproject_wait();
    assert_eq!(s.project().name, "Demo Song");
    let t = s.toast.clone().expect("the toast");
    assert!(t.alert);
    let (label, answer) = t.button.clone().unwrap();
    assert_eq!(label, SEE);
    let Answer::Skipped(list) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(t.text, format!("{NOT_READ} {}", list.rows.len()));
    assert_eq!(list.title, NOT_READ);
    assert!(list.rows.len() >= 4, "{:?}", list.rows);
    assert!(
        list.rows.iter().any(|(l, _)| l.contains("Old Verb")),
        "{:?}",
        list.rows
    );

    let parts = crate::toast::layout(&s, &g).unwrap();
    let (x, y) = middle(parts.button.unwrap());
    assert!(s.toast_press(&g, x, y));
    let p = s.popup.as_ref().expect("the list");
    assert_eq!(p.kind, PopupKind::Skipped);
    assert!(p.layout(g.size).rect.bottom() <= parts.rect.y);
    assert!(s.toast.is_some(), "the toast stays under its list");
}

#[test]
fn a_file_that_cannot_be_read_leaves_the_song_and_says_so() {
    let dir = folder("bad");
    let bad = dir.path().join("bad.dawproject");
    std::fs::write(&bad, b"not a zip").unwrap();
    let mut s = Studio::sample();
    let name = s.project().name.clone();
    assert!(s.dawproject_open(&bad));
    s.dawproject_wait();
    assert_eq!(s.project().name, name);
    let t = s.toast.clone().unwrap();
    assert!(t.alert);
    assert!(
        t.text.starts_with("bad.dawproject を開けません（"),
        "{}",
        t.text
    );
}

#[test]
fn one_at_a_time() {
    let dir = folder("one");
    let path = with_four_left_out(dir.path());
    let mut s = Studio::new(Song::empty());
    assert!(s.dawproject_open(&path));
    assert!(!s.dawproject_open(&path));
    assert!(!s.dawproject_export(&dir.path().join("x.dawproject")));
    s.dawproject_wait();
    assert!(s.dawproject_export(&dir.path().join("x.dawproject")));
    s.dawproject_wait();
    assert!(dir.path().join("x.dawproject").is_file());
}

#[test]
fn a_dragged_dawproject_names_itself_and_opens_where_it_is_dropped() {
    let dir = folder("drop");
    let path = with_four_left_out(dir.path());
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 900.0);
    let wav = dir.path().join("a.wav");
    assert!(s.drag_names(&[wav.clone(), path.clone()]));
    assert!(s.drop_hover(true));
    assert_eq!(s.drop_dawproject.as_deref(), Some("Demo Song.dawproject"));
    assert!(!s.drag_names(&[wav.clone(), path.clone()]), "no change");
    // Dragged out again: the words go.
    assert!(s.drop_hover(false));
    assert_eq!(s.drop_dawproject, None);
    assert!(s.drag_names(std::slice::from_ref(&path)));
    let (x, y) = middle(g.lanes);
    assert!(s.drop_files(&g, x, y, std::slice::from_ref(&path)));
    assert!(!s.drop_over);
    assert_eq!(s.drop_dawproject, None);
    assert!(s.dawproject.is_some());
    s.dawproject_wait();
    assert_eq!(s.project().name, "Demo Song");
    // Other files are not named.
    assert!(!s.drag_names(&[wav]));
    assert_eq!(s.drop_dawproject, None);
}

#[test]
fn the_list_is_as_wide_as_its_longest_row_and_scrolls_past_twelve() {
    let short = List {
        title: NOT_READ.into(),
        rows: vec![("シーン".into(), "4".into())],
    };
    assert_eq!(short.width(), MIN_W);
    assert_eq!(short.max_offset(), 0);
    let long = List {
        title: NOT_READ.into(),
        rows: (0..15)
            .map(|i| {
                (
                    format!("とても長い名前のトラック {i} の音源プラグイン"),
                    "Diva（VST2）".into(),
                )
            })
            .collect(),
    };
    assert!(long.width() > MIN_W);
    assert_eq!(long.max_offset(), 3);
    let cells = cells(&long, 3);
    assert_eq!(cells.len(), 1 + ROWS);
    assert_eq!(cells[1].text, "とても長い名前のトラック 3 の音源プラグイン");
    assert_eq!(cells[1].hint, "Diva（VST2）");
    assert!(cells.iter().all(|c| c.rect.right() <= long.width()));
}

#[test]
fn with_extension_adds_the_ending_once() {
    assert_eq!(
        with_extension(Path::new("a/b")),
        PathBuf::from("a/b.dawproject")
    );
    assert_eq!(
        with_extension(Path::new("a/b.DAWproject")),
        PathBuf::from("a/b.DAWproject")
    );
    assert_eq!(
        with_extension(Path::new("a/v1.2")),
        PathBuf::from("a/v1.2.dawproject")
    );
}

/// The project of the DAWproject README (Bitwig 5.0), with its Surge XT state.
fn readme_song(dir: &Path) -> PathBuf {
    let path = dir.join("Readme.dawproject");
    let mut zip = ZipWriter::new(std::fs::File::create(&path).unwrap());
    let xml = include_str!("../../../xirius-core/src/dawproject/testdata/bitwig-5.0-readme.xml");
    zip.add("project.xml", xml.as_bytes(), true).unwrap();
    zip.add(
        "plugins/d19b1f6e-bbb6-42fe-a6c9-54b41d97a05d.clap-preset",
        b"surge state",
        true,
    )
    .unwrap();
    zip.finish().unwrap();
    path
}

fn scanned_with(uid: &str) -> xirius_plugins::Catalog {
    xirius_plugins::Catalog {
        entries: vec![xirius_plugins::Entry {
            uid: uid.into(),
            name: "Other Synth".into(),
            vendor: "Other".into(),
            kind: xirius_core::model::PluginKind::Instrument,
            format: "clap".into(),
            category: String::new(),
            version: "1".into(),
            path: PathBuf::from("Other.clap"),
            ara: false,
        }],
        failures: Vec::new(),
    }
}

#[test]
fn a_plugin_this_pc_does_not_have_is_listed_and_kept_with_its_state() {
    let dir = folder("plugin");
    let path = readme_song(dir.path());
    let missing = "Bass の Surge XT（この PC に無い）";
    let (mut s, g) = studio_in(dir.path());
    s.use_plugins(scanned_with("com.example.other"));
    assert!(s.press(&g, Hit::Bar(BarButton::SongName)));
    assert!(press_cell(&mut s, &g, Action::OpenDawproject));
    let request = s.file_request.take().unwrap();
    s.finish_file_request(request, &path);
    s.dawproject_wait();
    assert_eq!(s.project().name, "Readme");
    let t = s.toast.clone().expect("the toast");
    let (_, Answer::Skipped(list)) = t.button.clone().unwrap() else {
        panic!("{t:?}");
    };
    assert!(
        list.rows.iter().any(|(l, _)| l == missing),
        "{:?}",
        list.rows
    );
    let p = s.project();
    let bass = p.tracks.values().find(|t| t.name == "Bass").unwrap();
    assert_eq!(bass.inserts[0].name, "Surge XT");
    assert_eq!(bass.inserts[0].uid, "org.surge-synth-team.surge-xt");
    assert!(bass.inserts[0].state.is_some());

    // The PC has it: not listed.
    let mut s = Studio::new(Song::empty());
    s.use_plugins(scanned_with("ORG.SURGE-SYNTH-TEAM.SURGE-XT"));
    assert!(s.dawproject_open(&path));
    s.dawproject_wait();
    let rows = s
        .toast
        .clone()
        .and_then(|t| t.button)
        .map(|(_, a)| match a {
            Answer::Skipped(list) => list.rows,
            a => panic!("{a:?}"),
        })
        .unwrap_or_default();
    assert!(rows.iter().all(|(l, _)| l != missing), "{rows:?}");

    // Never scanned: nothing to go by, so nothing is said to be missing.
    let mut s = Studio::new(Song::empty());
    s.use_plugins(xirius_plugins::Catalog {
        entries: Vec::new(),
        failures: Vec::new(),
    });
    assert!(s.dawproject_open(&path));
    s.dawproject_wait();
    let rows = s
        .toast
        .clone()
        .and_then(|t| t.button)
        .map(|(_, a)| match a {
            Answer::Skipped(list) => list.rows,
            a => panic!("{a:?}"),
        })
        .unwrap_or_default();
    assert!(rows.iter().all(|(l, _)| l != missing), "{rows:?}");
}
