//! CTX-018: the folder's "ステムを書き出す" row has two rows under it, the folder as one file and
//! the tracks inside one by one. The panel it opens writes the files for real.

use std::time::{Duration, Instant};

use serde_json::json;

use super::tests::{choose, enabled, labels, studio};
use crate::arrange::probe::folder;
use crate::export::Group;
use crate::popup::PopupKind;
use crate::studio::Studio;
use crate::tree::RowKind;
use xirius_core::id::FolderId;

const ONE: &str = "このフォルダーを 1 本に…";
const EACH: &str = "中のトラックごと…";

fn press(s: &mut Studio, g: &crate::studio::Geometry, f: FolderId) -> bool {
    let (x, y) = crate::arrange::probe::row_mid(s, g, RowKind::Folder(f));
    s.scene_context(g, x, y)
}

fn connect(s: &mut Studio, f: FolderId) {
    s.command(|song| {
        song.run("folder.bus.add", json!({ "id": f.to_string() }))
            .map(|_| ())
    });
    assert!(s.error.is_none(), "{:?}", s.error);
}

#[test]
fn the_folder_is_one_file_only_when_it_is_connected_to_a_bus() {
    let (mut s, g) = studio();
    let f = folder(&s, "Rhythm");
    assert!(press(&mut s, &g, f));
    assert!(choose(&mut s, &g, "ステムを書き出す"));
    assert_eq!(labels(&s), ["戻る", ONE, EACH]);
    assert!(!enabled(&s, ONE), "no bus, no single file");
    assert!(enabled(&s, EACH));
    s.popup = None;

    connect(&mut s, f);
    assert!(press(&mut s, &g, f));
    assert!(choose(&mut s, &g, "ステムを書き出す"));
    assert!(enabled(&s, ONE) && enabled(&s, EACH));
}

#[test]
fn the_single_file_row_opens_the_panel_with_the_folders_bus_and_leaves_the_song_alone() {
    let (mut s, g) = studio();
    let f = folder(&s, "Rhythm");
    connect(&mut s, f);
    let bus = s.project().folders[&f].bus.expect("the bus");
    assert!(press(&mut s, &g, f));
    let n = s.song.steps();
    assert!(choose(&mut s, &g, "ステムを書き出す"));
    assert!(choose(&mut s, &g, ONE));
    assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Export));
    assert!(s.export.stems);
    assert_eq!(s.export.group, Group::Selected);
    assert_eq!(
        (s.export.picked.clone(), s.export.folders.clone()),
        (vec![bus], vec![f])
    );
    assert_eq!(s.song.steps(), n);

    // The tracks one by one drop the folder.
    s.popup = None;
    assert!(press(&mut s, &g, f));
    assert!(choose(&mut s, &g, "ステムを書き出す"));
    assert!(choose(&mut s, &g, EACH));
    assert_eq!(s.export.picked, s.project().tracks_in(f));
    assert!(s.export.folders.is_empty());
}

#[test]
fn go_writes_one_file_named_after_the_folder_and_unticking_the_bus_drops_it() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-ctx-foldstem-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("ctx-foldstem");
    let _ = std::fs::remove_dir_all(&dir);
    let (mut s, g) = studio();
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    let f = folder(&s, "Rhythm");
    connect(&mut s, f);
    let bus = s.project().folders[&f].bus.expect("the bus");
    assert!(press(&mut s, &g, f));
    assert!(choose(&mut s, &g, "ステムを書き出す"));
    assert!(choose(&mut s, &g, ONE));

    let (folders, rest) = s.export.folder_stems(&s.project());
    assert_eq!((folders, rest), (vec![f], vec![]));
    // The bus unticked: no folder.
    assert!(s.export_action(crate::export::Act::Pick(bus)));
    assert_eq!(s.export.folder_stems(&s.project()).0, vec![]);
    assert!(s.export_action(crate::export::Act::Pick(bus)));

    assert!(s.export_action(crate::export::Act::Go));
    assert!(s.error.is_none(), "{:?}", s.error);
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end && s.export.jobs.iter().all(|j| j["state"] != "done") {
        s.tick();
        std::thread::sleep(Duration::from_millis(16));
    }
    let job = s.export.jobs.first().expect("a job");
    assert_eq!(job["state"], "done", "{job}");
    assert_eq!(job["args"]["folders"], json!([f.to_string()]));
    let files: Vec<String> = std::fs::read_dir(dir.join("Stems"))
        .unwrap()
        .filter_map(|e| Some(e.ok()?.file_name().to_string_lossy().into_owned()))
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with("_Rhythm.wav"), "{files:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
