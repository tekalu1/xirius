//! Audio files dropped on the window (UI-075): a track and a clip, the tempo said in one line,
//! one undo for the lot.

use std::path::PathBuf;

use xirius_core::model::ClipBody;

use super::*;
use crate::song::Song;

/// A folder of the test's own, removed when dropped (also when the test fails).
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-audio-{name}-"))
        .tempdir()
        .unwrap()
}

/// `secs` of a quiet tone as `name` in `dir`.
pub(crate) fn tone(dir: &Path, name: &str, secs: f64) -> PathBuf {
    let path = dir.join(name);
    let n = (48_000.0 * secs) as usize;
    let x: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
    xirius_session::export::write_wav(&path, &x, &x, 48_000, 16).unwrap();
    path
}

fn bpm(s: &Studio) -> f64 {
    s.project()
        .tempo
        .iter()
        .find(|t| t.tick == 0)
        .map_or(120.0, |t| t.bpm)
}

fn audio_clips(s: &Studio) -> Vec<(TrackId, String, Tick)> {
    let p = s.project();
    let mut out = Vec::new();
    for t in p.tracks.values() {
        for c in &t.clips {
            if let ClipBody::Audio { file, .. } = &c.body
                && Path::new(file).is_absolute()
            {
                out.push((t.id, file.clone(), c.start));
            }
        }
    }
    out
}

#[test]
fn a_loop_dropped_on_the_empty_song_is_a_track_with_its_tempo_and_one_undo() {
    let dir_tmp = folder("empty");
    let dir = dir_tmp.path().to_path_buf();
    let drums = tone(&dir, "Drums 92bpm.wav", 60.0 / 92.0 * 4.0);
    let mut s = Studio::new(Song::empty());
    let g = s.geometry(1600.0, 1000.0);
    let z = crate::start::zone(&g);
    let (x, y) = (z.x + z.w / 2.0, z.y + z.h / 2.0);
    assert!(s.drop_files(&g, x, y, std::slice::from_ref(&drums)));
    s.settle_browser_import();
    assert_eq!(s.error, None);
    assert_eq!(
        s.notice_text(),
        Some("「Drums 92bpm」を置き、曲のテンポを 92 BPM にしました")
    );
    assert_eq!(bpm(&s), 92.0);
    let p = s.project();
    let (t, c) = s.selected_clip.expect("the clip is selected");
    let track = &p.tracks[&t];
    assert_eq!(track.kind, TrackKind::Audio);
    assert_eq!(track.name, "Drums 92bpm");
    let clip = track.clips.iter().find(|x| x.id == c).unwrap();
    // One bar of the loop is one bar of the song: nothing was stretched.
    assert_eq!((clip.start, clip.length), (0, 4 * i64::from(p.ppq)));
    assert!(!s.start_shown());

    // One undo: no track, no clip, the tempo as it was.
    let g = s.geometry(1600.0, 1000.0);
    assert!(s.undo(&g));
    assert_eq!(bpm(&s), 120.0);
    assert!(s.start_shown());
    assert!(audio_clips(&s).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn on_an_audio_track_the_clip_goes_onto_it_at_the_bar_and_elsewhere_onto_a_new_track() {
    let dir_tmp = folder("lanes");
    let dir = dir_tmp.path().to_path_buf();
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let p = s.project();
    let audio = p
        .tracks
        .values()
        .find(|t| t.kind == TrackKind::Audio)
        .expect("an audio track")
        .id;
    let midi = p
        .tracks
        .values()
        .find(|t| t.kind == TrackKind::Instrument)
        .expect("an instrument track")
        .id;
    let row = *s.tree().row_of(RowKind::Track(audio)).unwrap();
    let y = s.row_y(&g, &row) + row.h / 2.0;
    let bar3 = p.time_map().bar_start(3);
    let x = s.tick_x(&g, bar3 + 600);
    assert_eq!(s.audio_track_at(&g, x, y), Some(audio));

    let vox = tone(&dir, "Vox.wav", 0.5);
    let tracks = p.tracks.len();
    assert!(s.drop_files(&g, x, y, std::slice::from_ref(&vox)));
    s.settle_browser_import();
    assert_eq!(s.error, None);
    assert_eq!(s.project().tracks.len(), tracks);
    assert_eq!(
        audio_clips(&s),
        [(audio, vox.to_string_lossy().into(), bar3)]
    );
    assert_eq!(s.notice_text(), Some("「Vox」を置きました"));
    assert_eq!(s.selected_clip.map(|c| c.0), Some(audio));

    // Over an instrument track: a new audio track. Two files: the second on its own too.
    let row = *s.tree().row_of(RowKind::Track(midi)).unwrap();
    let y = s.row_y(&g, &row) + row.h / 2.0;
    assert_eq!(s.audio_track_at(&g, x, y), None);
    let hat = tone(&dir, "Hat 140bpm.wav", 0.25);
    assert!(s.drop_files(&g, x, y, &[hat.clone(), vox.clone()]));
    s.settle_browser_import();
    assert_eq!(s.project().tracks.len(), tracks + 2);
    // The hat has a tempo of its own: the line offers to make it follow the song's.
    let toast = s.toast_shown().expect("the line stays with the offer");
    assert_eq!(toast.text, "音声ファイル 2 個を置きました");
    assert!(matches!(&toast.button, Some((_, crate::toast::Answer::Follow(c))) if c.len() == 1));
    s.toast = None;
    // The song has clips: its tempo stays.
    assert_eq!(bpm(&s), 120.0);

    // One file over nothing: its tempo is said, not taken, and the clip keeps its own sound
    // until it is made to follow the song's (EDT-007).
    let g = s.geometry(1600.0, 1000.0);
    assert!(s.import_audio_at(&g, x, g.lanes.bottom() - 2.0, &[hat.as_path()]));
    s.settle_browser_import();
    let toast = s.toast_shown().expect("the line stays with the offer");
    assert_eq!(
        toast.text,
        "「Hat 140bpm」を置きました（ファイルのテンポは 140 BPM。曲のテンポは変えていません）"
    );
    assert_eq!(toast.button.as_ref().unwrap().0, "曲のテンポに合わせる");
    assert_eq!(bpm(&s), 120.0);
    let (_, hat_clip) = s.selected_clip.expect("the placed clip");
    let process = |s: &Studio| match &s.project().clip(hat_clip).unwrap().1.body {
        ClipBody::Audio { process, .. } => *process,
        ClipBody::Midi { .. } => unreachable!(),
    };
    assert_eq!(
        (process(&s).tempo, process(&s).follow),
        (Some(140.0), false)
    );
    let length = s.project().clip(hat_clip).unwrap().1.length;
    s.toast_answer();
    assert!(process(&s).follow);
    // At the song's 120 the 140 loop plays slower, so it is longer.
    assert!(s.project().clip(hat_clip).unwrap().1.length > length);
    assert!(s.undo(&g));
    assert!(!process(&s).follow);

    // Each file was a step of its own.
    assert!(s.undo(&g));
    assert_eq!(s.project().tracks.len(), tracks + 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_cannot_be_read_says_why_and_changes_nothing() {
    let dir_tmp = folder("bad");
    let dir = dir_tmp.path().to_path_buf();
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let bad = dir.join("broken.flac");
    std::fs::write(&bad, b"fLaC-not-really").unwrap();
    let before = s.project();
    assert!(s.import_audio_at(&g, g.lanes.x + 10.0, g.lanes.y + 10.0, &[bad.as_path()]));
    s.settle_browser_import();
    assert!(
        s.error
            .as_deref()
            .is_some_and(|e| e.starts_with("「broken.flac」: ")),
        "{:?}",
        s.error
    );
    assert_eq!(*s.project(), *before);
    let _ = std::fs::remove_dir_all(&dir);
}
