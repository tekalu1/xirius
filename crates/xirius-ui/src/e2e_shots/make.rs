//! Stages 0–14: a new song, its tempo and meter, the tracks and their instrument, notes drawn,
//! moved and quantized, drum hits, the song saved and a file dragged in from the browser.

use super::{
    Done, Studio1, cell, cell_where, check, click, click_n, drag, drag_m, keys, made, memo, mid,
    press, read, row_of, song_dir, song_json, spot, track_of, tracks,
};
use crate::{
    browser::{BrowserHit, RailTab},
    popup::Action,
    start::Entry,
    studio::{DrawerTab, FileRequest, Hit},
    tools::Tool,
    topbar::BarButton,
};
use gpui::{App, Modifiers, Window};
use std::path::{Path, PathBuf};
use xirius_core::{id::TrackId, model::PluginKind};

/// The VST3 instruments tried, in order, in the standard folder; `XIRIUS_TEST_VST3_INSTRUMENT`
/// names another bundle there.
const INSTRUMENTS: [&str; 2] = ["sforzando.vst3", "Vital.vst3"];
/// The file dragged in from the browser.
const WAV: &str = "e2e-tone.wav";

pub(super) fn play(n: u8, s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    match n {
        0 => new_song(s, w, cx),
        1 => {
            let before = tracks(s);
            press(s, Hit::Start(Entry::Midi), w, cx)?;
            let t = made(s, &before)?;
            memo(|m| m.keys = Some(t));
            let p = s.borrow().project();
            let track = &p.tracks[&t];
            check(
                crate::instrument::current(track).is_some(),
                "the track has no instrument",
            )?;
            check(track.armed, "the track is not armed")?;
            check(!s.borrow().start_shown(), "the start screen stays")?;
            Ok(format!("track={} armed", track.name))
        }
        2 => {
            let p = spot(s, w, Hit::Bar(BarButton::Tempo))?;
            click_n(p, 2, w, cx);
            keys("1 2 8 enter", w, cx);
            let p = s.borrow().project();
            let bpm = p.tempo.first().map_or(0.0, |t| t.bpm);
            let shown = s.borrow().bar_view().bpm;
            check(bpm == 128.0, format!("the tempo is {bpm}"))?;
            check(shown.starts_with("128"), format!("the bar shows {shown}"))?;
            Ok(format!("bpm={bpm} shown={shown}"))
        }
        3 => {
            press(s, Hit::Bar(BarButton::Meter), w, cx)?;
            cell(s, "3/4", w, cx)?;
            let p = s.borrow().project();
            let m = p.timesig.first().map(|m| (m.num, m.den));
            check(m == Some((3, 4)), format!("the meter is {m:?}"))?;
            check(s.borrow().popup.is_none(), "the list stays open")?;
            Ok("meter=3/4".into())
        }
        4 => {
            let before = tracks(s);
            press(s, Hit::AddTrack, w, cx)?;
            cell(s, "インストゥルメント", w, cx)?;
            let t = made(s, &before)?;
            memo(|m| m.synth = Some(t));
            let p = s.borrow().project();
            let has = crate::instrument::current(&p.tracks[&t]).map(|i| i.name.clone());
            Ok(format!("track={} instrument={has:?}", p.tracks[&t].name))
        }
        5 => instrument_list(s, w, cx),
        6 => pick_plugin(s, w, cx),
        7 => {
            let t = track_of(|m| m.synth, "instrument")?;
            let part = draw_part(s, t, 1, 3, w, cx)?;
            Ok(format!("part ticks={part}"))
        }
        8 => {
            tool(s, Tool::Draw, w, cx)?;
            let before = notes(s, w)?.len();
            let spots = read(s, w, |s, g| {
                let c = s.roll_ctx(g).ok_or("the roll is not open")?;
                let v = s.roll.view(&c);
                let ppq = i64::from(s.project().ppq);
                // Three rows in the middle of the grid, a beat apart.
                let top = v.pitch_at(v.grid.y + v.grid.h / 2.0).clamp(4, 120) as u8;
                Ok::<_, String>(
                    [(0, top), (1, top - 2), (2, top - 4)]
                        .map(|(beat, pitch)| (v.x(beat * ppq + 1), v.y(pitch) + v.row_h / 2.0)),
                )
            })?;
            for p in spots {
                click(p, w, cx);
            }
            let now = notes(s, w)?;
            check(
                now.len() == before + 3,
                format!("{} notes after three presses", now.len()),
            )?;
            Ok(format!("notes={}", now.len()))
        }
        9 => {
            tool(s, Tool::Select, w, cx)?;
            let (from, to, id, start) = read(s, w, |s, g| {
                let c = s.roll_ctx(g).ok_or("the roll is not open")?;
                let v = s.roll.view(&c);
                let n = c.notes().iter().min_by_key(|n| n.start).ok_or("no note")?;
                let p = (
                    v.x(n.start) + v.px(n.length).min(12.0) / 2.0,
                    v.y(n.pitch) + v.row_h / 2.0,
                );
                let ppq = i64::from(s.project().ppq);
                Ok::<_, String>((p, (p.0 + v.px(ppq), p.1), n.id, n.start))
            })?;
            drag(from, to, w, cx);
            let now = notes(s, w)?;
            let n = now.iter().find(|n| n.id == id).ok_or("the note is gone")?;
            let ppq = i64::from(s.borrow().project().ppq);
            check(
                n.start == start + ppq,
                format!("the note went from {start} to {}", n.start),
            )?;
            Ok(format!("start {start} -> {}", n.start))
        }
        10 => quantize(s, w, cx),
        11 => {
            let t = track_of(|m| m.keys, "MIDI")?;
            draw_part(s, t, 1, 2, w, cx)?;
            press(s, Hit::RollView(true), w, cx)?;
            let drums = read(s, w, |s, g| s.roll_ctx(g).map(|c| s.roll.view(&c).drums()));
            check(drums == Some(true), format!("the drum view is {drums:?}"))?;
            Ok("drum view".into())
        }
        12 => {
            tool(s, Tool::Draw, w, cx)?;
            let before = notes(s, w)?.len();
            let spots = read(s, w, |s, g| {
                let c = s.roll_ctx(g).ok_or("the roll is not open")?;
                let v = s.roll.view(&c);
                let ppq = i64::from(s.project().ppq);
                // The two lowest rows the grid shows (the kick's and the next); rows above
                // the grid's top are scrolled away.
                let shown: Vec<u8> = v
                    .rows
                    .pitches()
                    .filter(|p| v.y(*p) >= v.grid.y && v.y(*p) + v.row_h <= v.grid.bottom())
                    .collect();
                check(shown.len() >= 2, format!("{} drum rows shown", shown.len()))?;
                let mut out = Vec::new();
                for pitch in &shown[shown.len() - 2..] {
                    let pitch = *pitch;
                    for beat in [0, 1, 2] {
                        out.push((v.x(beat * ppq + 1), v.y(pitch) + v.row_h / 2.0));
                    }
                }
                Ok::<_, String>(out)
            })?;
            for p in &spots {
                click(*p, w, cx);
            }
            let now = notes(s, w)?;
            check(
                now.len() == before + spots.len(),
                format!("{} hits after {} presses", now.len(), spots.len()),
            )?;
            let pitches: std::collections::BTreeSet<_> = now.iter().map(|n| n.pitch).collect();
            Ok(format!("hits={} pitches={pitches:?}", now.len()))
        }
        13 => {
            press(s, Hit::Bar(BarButton::SongName), w, cx)?;
            cell(s, "別名で保存…", w, cx)?;
            let path = song_path();
            answer(s, FileRequest::SaveAs, &path)?;
            let s = s.borrow();
            check(s.song.is_saved(), "the song is not saved")?;
            check(
                path.join("project.json").exists() || std::fs::read_dir(&path).is_ok(),
                "no folder",
            )?;
            Ok(format!("saved to {}", path.display()))
        }
        14 => drag_file(s, w, cx),
        _ => Err(format!("no stage {n}")),
    }
}

/// The folder the song is saved in.
pub(super) fn song_path() -> PathBuf {
    song_dir().join("song")
}

/// Answers the open or save window the screen asked for, with `path`.
pub(super) fn answer(s: &Studio1, want: FileRequest, path: &Path) -> Result<(), String> {
    let asked = s.borrow_mut().file_request.take();
    check(
        asked.as_ref() == Some(&want),
        format!("the screen asked for {asked:?}, not {want:?}"),
    )?;
    s.borrow_mut().finish_file_request(want, path);
    Ok(())
}

/// Picks a tool with its button in the bar (digits would play notes while the keys play).
pub(super) fn tool(s: &Studio1, t: Tool, w: &mut Window, cx: &mut App) -> Result<(), String> {
    press(s, Hit::Bar(BarButton::Tool(t)), w, cx)?;
    let now = s.borrow().tool;
    check(now == t, format!("the tool is {now:?}"))
}

/// The drawer shut with its tab, for room under the tracks.
pub(super) fn shut_drawer(s: &Studio1, w: &mut Window, cx: &mut App) -> Result<(), String> {
    if s.borrow().drawer_open {
        let tab = s.borrow().tab;
        press(s, Hit::Tab(tab), w, cx)?;
    }
    check(!s.borrow().drawer_open, "the drawer stays open")
}

/// The notes of the clip in the roll.
fn notes(s: &Studio1, w: &Window) -> Result<Vec<xirius_core::model::Note>, String> {
    read(s, w, |s, g| {
        s.roll_ctx(g)
            .map(|c| c.notes().to_vec())
            .ok_or_else(|| "the roll is not open".to_string())
    })
}

fn new_song(s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    // The run checks what it presses where things come to rest; sliding panels would put
    // them elsewhere for a few frames on a busy PC.
    s.borrow().motion.enabled.set(false);
    let _ = std::fs::remove_dir_all(song_dir());
    std::fs::create_dir_all(song_dir()).map_err(|e| e.to_string())?;
    press(s, Hit::Bar(BarButton::SongName), w, cx)?;
    cell(s, "新規", w, cx)?;
    let st = s.borrow();
    check(st.start_shown(), "the start screen is not shown")?;
    check(!st.song.can_undo(), "a new song has steps to undo")?;
    drop(st);
    memo(|m| m.empty = Some(song_json(s)));
    Ok("start screen".into())
}

/// The instrument list of the new track, with this PC's plugin instrument in it.
fn instrument_list(s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    let t = track_of(|m| m.synth, "instrument")?;
    let entries = test_instrument();
    let found = entries
        .iter()
        .find(|e| e.kind == PluginKind::Instrument)
        .map(|e| format!("{} ({})", e.name, e.format));
    s.borrow_mut().use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    press(s, Hit::Tab(DrawerTab::Mixer), w, cx)?;
    check(s.borrow().mixer_shown(), "the mixer is not shown")?;
    let p = s.borrow().project();
    match crate::instrument::current(&p.tracks[&t]) {
        None => press(
            s,
            Hit::Mixer(crate::mixer::MixerHit::AddInstrument(t)),
            w,
            cx,
        )?,
        Some(i) => {
            let at = spot(s, w, Hit::Mixer(crate::mixer::MixerHit::Open(t, i.id)))?;
            super::right_click(at, w, cx);
            cell(s, "差し替える…", w, cx)?;
        }
    }
    let kind = s.borrow().popup.as_ref().map(|p| p.kind);
    check(
        kind == Some(crate::popup::PopupKind::Instrument(t)),
        format!("the open list is {kind:?}"),
    )?;
    Ok(format!(
        "plugin={}",
        found.unwrap_or_else(|| "none on this PC".into())
    ))
}

/// This PC's VST3 instrument, read as the scan would.
fn test_instrument() -> Vec<xirius_plugins::Entry> {
    let wanted = std::env::var("XIRIUS_TEST_VST3_INSTRUMENT").ok();
    let names: Vec<&str> = match &wanted {
        Some(n) => vec![n.as_str()],
        None => INSTRUMENTS.to_vec(),
    };
    names
        .iter()
        .map(|n| xirius_vst3::default_vst3_dir().join(n))
        .filter(|p| p.exists())
        .find_map(|p| xirius_plugins::probe(&p).ok())
        .unwrap_or_default()
}

fn pick_plugin(s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    let t = track_of(|m| m.synth, "instrument")?;
    let uid = s
        .borrow_mut()
        .plugin_catalog()
        .entries
        .iter()
        .find_map(|e| {
            (e.kind == PluginKind::Instrument && e.format == "vst3").then(|| e.uid.clone())
        });
    let Some(uid) = uid else {
        // No plugin instrument on this PC: the built-in one is picked from the same list.
        let to = format!("builtin:{}", crate::instrument::DEFAULT);
        cell_where(
            s,
            "the built-in instrument",
            |c| {
                c.action
                    .as_ref()
                    .is_some_and(|a| *a == Action::Pick(to.clone()))
            },
            w,
            cx,
        )?;
        let p = s.borrow().project();
        let i = crate::instrument::current(&p.tracks[&t]).ok_or("no instrument")?;
        return Ok(format!("skip-plugin built-in={}", i.name));
    };
    cell_where(
        s,
        &uid,
        |c| {
            c.action
                .as_ref()
                .is_some_and(|a| *a == Action::Pick(uid.clone()))
        },
        w,
        cx,
    )?;
    let p = s.borrow().project();
    let i = crate::instrument::current(&p.tracks[&t]).ok_or("no instrument")?;
    check(
        i.format == "vst3" && i.uid == uid,
        format!("the instrument is {} ({})", i.name, i.format),
    )?;
    Ok(format!("instrument={} format={}", i.name, i.format))
}

/// A part drawn with the pencil on `t`'s lane over bars `from`..`to`, then opened in the roll
/// with a double press. Gives the part's length.
fn draw_part(
    s: &Studio1,
    t: TrackId,
    from: u32,
    to: u32,
    w: &mut Window,
    cx: &mut App,
) -> Result<i64, String> {
    tool(s, Tool::Draw, w, cx)?;
    let clips = |s: &Studio1| s.borrow().project().tracks[&t].clips.len();
    let before = clips(s);
    let (a, b) = read(s, w, |s, g| {
        let tm = s.project().time_map();
        Ok::<_, String>((
            super::lane_at(s, g, t, tm.bar_start(from))?,
            super::lane_at(s, g, t, tm.bar_start(to))?,
        ))
    })?;
    drag((a.0 + 2.0, a.1), (b.0 - 2.0, b.1), w, cx);
    check(
        clips(s) == before + 1,
        format!("{} parts after the drag", clips(s)),
    )?;
    tool(s, Tool::Select, w, cx)?;
    let (clip, at) = read(s, w, |s, g| {
        let p = s.project();
        let c = p.tracks[&t]
            .clips
            .iter()
            .max_by_key(|c| c.start)
            .ok_or("no part")?;
        let r = s.clip_rect(g, &row_of(s, t)?, c);
        Ok::<_, String>((c.clone(), mid(r)))
    })?;
    click_n(at, 2, w, cx);
    let open = read(s, w, |s, g| s.roll_ctx(g).map(|c| c.clip.id));
    check(
        open == Some(clip.id),
        format!("the roll shows {open:?}, not the part"),
    )?;
    Ok(clip.length)
}

/// A note moved off the grid with Shift, then every note quantized with Ctrl+A and Q.
fn quantize(s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    let (from, to, id, grid) = read(s, w, |s, g| {
        let c = s.roll_ctx(g).ok_or("the roll is not open")?;
        let v = s.roll.view(&c);
        let n = c.notes().iter().max_by_key(|n| n.start).ok_or("no note")?;
        let p = (
            v.x(n.start) + v.px(n.length).min(12.0) / 2.0,
            v.y(n.pitch) + v.row_h / 2.0,
        );
        let ppq = i64::from(s.project().ppq);
        // A third of a sixteenth on: off every grid the song can have.
        Ok::<_, String>((p, (p.0 + v.px(ppq / 12), p.1), n.id, ppq / 4))
    })?;
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    drag_m(from, to, shift, w, cx);
    let off = notes(s, w)?
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.start)
        .ok_or("the note is gone")?;
    check(
        off % grid != 0,
        format!("Shift kept the note on the grid at {off}"),
    )?;
    keys("ctrl-a q", w, cx);
    let now = notes(s, w)?;
    let at = now
        .iter()
        .find(|n| n.id == id)
        .map(|n| n.start)
        .ok_or("the note is gone")?;
    let loose: Vec<_> = now
        .iter()
        .filter(|n| n.start % grid != 0)
        .map(|n| n.start)
        .collect();
    check(loose.is_empty(), format!("off the grid after Q: {loose:?}"))?;
    Ok(format!("{off} -> {at}, notes={}", now.len()))
}

/// A tone written to a folder, the folder added to the browser's places with its ＋, and the
/// file dragged from the browser onto the lanes under the tracks.
fn drag_file(s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    let dir = song_dir().join("place");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(WAV);
    std::fs::write(&file, tone(48_000, 4.0)).map_err(|e| e.to_string())?;
    shut_drawer(s, w, cx)?;
    // The panel shows the instruments' tab from before: the files' tab is pressed.
    let showing = |s: &Studio1| s.borrow().browser.open && s.borrow().browser.tab == RailTab::Files;
    if !showing(s) {
        press(s, Hit::Rail(RailTab::Files), w, cx)?;
    }
    check(showing(s), "the files are not shown")?;
    press(s, Hit::Browser(BrowserHit::LocAdd), w, cx)?;
    answer(s, FileRequest::Place, &dir)?;
    let before = tracks(s);
    let (from, to) = read(s, w, |s, g| {
        let i = crate::import_shots::file_row(s, g, WAV).ok_or("the file is not in the list")?;
        let from = s
            .browser
            .center_of(&s.browser.layout(g.panel), BrowserHit::File(i))
            .ok_or("the file's row is not on the screen")?;
        let last = s.tree().rows.last().ok_or("no rows")?;
        let y = s.row_y(g, last) + last.h + 30.0;
        let x = s.tick_x(g, s.project().time_map().bar_start(2));
        check(g.lanes.contains(x, y), "no room under the tracks")?;
        Ok::<_, String>((from, (x, y)))
    })?;
    drag(from, to, w, cx);
    check(
        s.borrow().browser.file_drag.is_none(),
        "the drag did not end",
    )?;
    let t = made(s, &before)?;
    memo(|m| m.audio = Some(t));
    let p = s.borrow().project();
    let track = &p.tracks[&t];
    check(
        track.clips.len() == 1,
        format!("{} clips", track.clips.len()),
    )?;
    let bar2 = p.time_map().bar_start(2);
    let start = track.clips[0].start;
    Ok(format!(
        "track={} clip at {start} (bar 2 = {bar2})",
        track.name
    ))
}

/// A mono 16-bit WAV of a 440 Hz sine at -12 dBFS.
pub(super) fn tone(rate: u32, secs: f32) -> Vec<u8> {
    let n = (rate as f32 * secs) as u32;
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + n * 2).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend((rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((n * 2).to_le_bytes());
    let amp = 0.25 * f32::from(i16::MAX);
    for i in 0..n {
        let v = (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin() * amp;
        wav.extend((v as i16).to_le_bytes());
    }
    wav
}
