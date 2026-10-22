//! Stages 25–33: the song saved, closed and opened again, written as WAV, MP3 and DAWproject,
//! a take recorded from the test input, and every step undone and done again.

use super::make::{answer, shut_drawer, song_path};
use super::{Done, Studio1, cell, check, differences, keys, memo, press, song_json, track_of};
use crate::{
    export::State,
    studio::{FileRequest, Hit},
    topbar::BarButton,
};
use gpui::{App, Window};
use std::path::{Path, PathBuf};

/// The most steps the undo and redo stages press for.
const MOST_STEPS: usize = 1000;

pub(super) fn play(n: u8, s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    match n {
        25 => {
            keys("escape", w, cx);
            keys("ctrl-s", w, cx);
            crate::shot::run_for(&mut s.borrow_mut(), 10.0, |s| s.saving.is_none());
            check(s.borrow().song.is_saved(), "Ctrl+S left the song unsaved")?;
            let file = song_path().join("project.json");
            check(file.exists(), format!("no {}", file.display()))?;
            memo(|m| m.closed = Some(song_json(s)));
            Ok(format!("saved {}", file.display()))
        }
        26 => {
            press(s, Hit::Bar(BarButton::SongName), w, cx)?;
            cell(s, "新規", w, cx)?;
            crate::shot::run_for(&mut s.borrow_mut(), 10.0, |s| s.start_shown());
            let st = s.borrow();
            check(st.start_shown(), "the start screen is not shown")?;
            check(!st.song.can_undo(), "the new song has steps to undo")?;
            drop(st);
            let empty = memo(|m| m.empty.clone()).ok_or("no new song")?;
            let diff = differences(&empty, &song_json(s));
            check(diff.is_empty(), format!("the new song differs at {diff:?}"))?;
            Ok("closed".into())
        }
        27 => {
            press(s, Hit::Bar(BarButton::SongName), w, cx)?;
            cell(s, "開く…", w, cx)?;
            answer(s, FileRequest::Open, &song_path())?;
            let now = song_json(s);
            let was = memo(|m| m.closed.clone()).ok_or("no song from the save")?;
            let diff = differences(&was, &now);
            check(diff.is_empty(), format!("opened differs at {diff:?}"))?;
            let p = s.borrow().project();
            Ok(format!("tracks={} same as saved", p.tracks.len()))
        }
        28 => {
            let before = written("wav");
            export(s, "WAV", w, cx)?;
            let file = newest(&before, "wav")?;
            measure(s, &file)
        }
        29 => {
            press(s, Hit::Bar(BarButton::Export), w, cx)?;
            cell(s, "MP3", w, cx)?;
            let view = s.borrow().export_view();
            if view.no_mp3() {
                check(!view.can_go(), "書き出す is live without an encoder")?;
                keys("escape", w, cx);
                let why = view.mp3_error.unwrap_or_default();
                return Ok(format!("skip-mp3 no encoder: {why}"));
            }
            let before = written("mp3");
            cell(s, "書き出す", w, cx)?;
            wait_export(s)?;
            keys("escape", w, cx);
            let file = newest(&before, "mp3")?;
            measure(s, &file)
        }
        30 => {
            press(s, Hit::Bar(BarButton::SongName), w, cx)?;
            cell(s, "DAWproject として書き出す…", w, cx)?;
            let out = super::song_dir().join("song-out");
            answer(s, FileRequest::ExportDawproject, &out)?;
            crate::shot::run_for(&mut s.borrow_mut(), 60.0, |s| s.dawproject.is_none());
            let toast = s.borrow().toast.clone();
            let done = toast.as_ref().is_some_and(|t| t.done);
            check(done, format!("the toast says {:?}", toast.map(|t| t.text)))?;
            let file =
                std::fs::File::open(out.with_extension("dawproject")).map_err(|e| e.to_string())?;
            let zip = xirius_core::dawproject::zip::ZipReader::new(std::io::BufReader::new(file))?;
            let names = zip.names().to_vec();
            check(
                names.iter().any(|n| n == "project.xml"),
                format!("no project.xml in {names:?}"),
            )?;
            let audio = names.iter().filter(|n| n.starts_with("audio/")).count();
            Ok(format!("entries={} audio={audio}", names.len()))
        }
        31 => {
            let t = track_of(|m| m.audio, "audio")?;
            shut_drawer(s, w, cx)?;
            let before = s.borrow().project().tracks[&t].clips.len();
            press(s, Hit::Arm(t), w, cx)?;
            check(
                s.borrow().project().tracks[&t].armed,
                "the track is not armed",
            )?;
            press(s, Hit::Bar(BarButton::Record), w, cx)?;
            check(s.borrow().transport.recording, "not recording")?;
            // What the start said (a track left out of the take, say); the stop's finish_take
            // clears `error`, so it is kept here for the check below.
            let said = s.borrow().error.clone();
            crate::shot::run_for(&mut s.borrow_mut(), 1.5, |_| false);
            press(s, Hit::Bar(BarButton::Stop), w, cx)?;
            crate::shot::run_for(&mut s.borrow_mut(), 10.0, |s| {
                s.project().tracks[&t].clips.len() > before
            });
            let error = s.borrow().error.clone();
            let p = s.borrow().project();
            let clips = &p.tracks[&t].clips;
            check(
                clips.len() > before,
                format!(
                    "{} clips after the take; error {error:?}; said {said:?}",
                    clips.len()
                ),
            )?;
            let take = clips.iter().max_by_key(|c| c.id).map(|c| c.length);
            press(s, Hit::Arm(t), w, cx)?;
            Ok(format!(
                "clips {before} -> {} take ticks={take:?}",
                clips.len()
            ))
        }
        32 => {
            memo(|m| m.full = Some(song_json(s)));
            let mut steps = 0;
            while s.borrow().song.can_undo() && steps < MOST_STEPS {
                keys("ctrl-z", w, cx);
                steps += 1;
            }
            memo(|m| m.steps = steps);
            check(
                !s.borrow().song.can_undo(),
                format!("still undoable after {steps}"),
            )?;
            let now = song_json(s);
            // The history begins where the song was opened (stage 27).
            let opened = memo(|m| m.closed.clone()).ok_or("no saved song")?;
            let empty = memo(|m| m.empty.clone()).ok_or("no new song")?;
            let to_opened = differences(&opened, &now);
            let to_empty = differences(&empty, &now);
            Ok(format!(
                "undone {steps} steps; differs from the opened song at {} places {:?}, from the new song at {} places",
                to_opened.len(),
                to_opened.iter().take(6).collect::<Vec<_>>(),
                to_empty.len(),
            ))
        }
        33 => {
            let mut steps = 0;
            while s.borrow().song.can_redo() && steps < MOST_STEPS {
                keys("ctrl-y", w, cx);
                steps += 1;
            }
            let undone = memo(|m| m.steps);
            check(
                !s.borrow().song.can_redo(),
                format!("still redoable after {steps}"),
            )?;
            check(steps == undone, format!("redone {steps} of {undone} steps"))?;
            let full = memo(|m| m.full.clone()).ok_or("no song before the undoing")?;
            let diff = differences(&full, &song_json(s));
            check(diff.is_empty(), format!("redone differs at {diff:?}"))?;
            Ok(format!("redone {steps} steps, the song is as it was"))
        }
        _ => Err(format!("no stage {n}")),
    }
}

/// A mixdown of the whole song in `format`, from the export panel.
fn export(s: &Studio1, format: &str, w: &mut Window, cx: &mut App) -> Result<(), String> {
    press(s, Hit::Bar(BarButton::Export), w, cx)?;
    for text in ["ミックスダウン", "曲全体", format, "書き出す"] {
        cell(s, text, w, cx)?;
    }
    wait_export(s)?;
    keys("escape", w, cx);
    Ok(())
}

fn wait_export(s: &Studio1) -> Result<(), String> {
    crate::shot::run_for(&mut s.borrow_mut(), 60.0, |s| !s.export.running());
    let view = s.borrow().export_view();
    let card = view.cards.first().ok_or("no card")?;
    check(
        card.state == State::Done,
        format!("the card is {:?}: {}", card.state, card.error),
    )
}

fn mixdown() -> PathBuf {
    song_path().join("Mixdown")
}

/// The files of this kind in the mixdown folder.
fn written(ext: &str) -> Vec<PathBuf> {
    std::fs::read_dir(mixdown())
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
                .collect()
        })
        .unwrap_or_default()
}

fn newest(before: &[PathBuf], ext: &str) -> Result<PathBuf, String> {
    let now = written(ext);
    let new: Vec<_> = now
        .iter()
        .filter(|p| !before.contains(p))
        .cloned()
        .collect();
    match new.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err(format!(
            "{} new .{ext} files in {}",
            new.len(),
            mixdown().display()
        )),
    }
}

/// The file's length against the song's, and its loudness.
fn measure(s: &Studio1, file: &Path) -> Done {
    let d = xirius_session::decode::decode(file)?;
    let secs = d.left.len() as f64 / f64::from(d.rate);
    let p = s.borrow().project();
    let end = s.borrow().export_view().end;
    let want = xirius_core::tempo::TempoMap::from_project(&p).seconds_at(end);
    let (lufs, peak) = xirius_session::export::loudness(&d.left, &d.right, d.rate);
    check(
        (secs - want).abs() < 0.05 || (secs > want && secs < want + 10.0),
        format!("{secs:.3} s long, the song is {want:.3} s"),
    )?;
    let lufs = lufs.ok_or("silent")?;
    check(lufs.is_finite() && lufs > -70.0, format!("{lufs} LUFS"))?;
    Ok(format!(
        "{} {secs:.3} s (song {want:.3} s) {lufs:.1} LUFS peak {:.1} dBTP",
        file.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default(),
        peak.unwrap_or(f64::NAN)
    ))
}
