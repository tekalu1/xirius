//! `--script import`: the file browser on real folders and files put on the arrangement
//! (BRW-002, BRW-005 to BRW-008, BRW-015, UI-017, UI-046). Everything it reads is made first in
//! a scratch folder: a place of its own with WAV and MIDI files, and the song beside it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use xirius_core::model::TrackKind;
use xirius_session::peaks;

use crate::browser::{BRow, BrowserHit, RailTab};
use crate::shot::{Act, Shot, Target};
use crate::studio::{FileRequest, Geometry, Hit, Studio};
use crate::wave::{self, Look};

pub const IMPORT: &[Shot] = &[
    // A place of its own beside the song's tab, its folders before its files.
    Shot {
        name: "m01-place-tab",
        acts: &[
            Act::Import(0),
            Act::Import(1),
            Act::Press(Target::Rail(RailTab::Files)),
        ],
    },
    // A folder opened in the tree.
    Shot {
        name: "m02-tree",
        acts: &[Act::Press(Target::Browser(BrowserHit::Dir(1)))],
    },
    // A file chosen: it plays, its waveform lit as far as it has played.
    Shot {
        name: "m03-audition",
        acts: &[
            Act::Press(Target::Browser(BrowserHit::File(2))),
            Act::Import(2),
        ],
    },
    // The search looks inside closed folders too.
    Shot {
        name: "m04-search",
        acts: &[Act::Import(3)],
    },
    // A file held over an audio track: the chip says where it goes.
    Shot {
        name: "m05-drag",
        acts: &[Act::Import(4)],
    },
    // Let go: a clip of the file's own waveform on that track.
    Shot {
        name: "m06-dropped",
        acts: &[Act::Import(5)],
    },
    // One undo takes it away again.
    Shot {
        name: "m07-undo",
        acts: &[Act::Key("ctrl-z")],
    },
    // 「ファイルから…」: each file on a track of its own at the playhead.
    Shot {
        name: "m08-from-file",
        acts: &[Act::Import(6)],
    },
    // A folder given a tab with the plus; the cross on the shown tab takes it away.
    Shot {
        name: "m09-tab-added",
        acts: &[
            Act::Import(7),
            Act::Hover(Target::Browser(BrowserHit::LocRemove(2))),
        ],
    },
    Shot {
        name: "m10-tab-removed",
        acts: &[Act::Press(Target::Browser(BrowserHit::LocRemove(2)))],
    },
];

/// The name the place of [`IMPORT`] is shown under.
pub const PLACE: &str = "素材";

/// A run's folder, removed when the thread that played the run ends (a test's thread ends also
/// when the test fails).
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

thread_local! {
    static DIR: RefCell<Option<Folder>> = const { RefCell::new(None) };
}

/// Where the run that is playing keeps its files: one folder a run, so tests do not meet.
pub fn dir() -> PathBuf {
    DIR.with(|d| {
        d.borrow_mut()
            .get_or_insert_with(|| {
                static RUN: AtomicUsize = AtomicUsize::new(0);
                let n = RUN.fetch_add(1, Ordering::Relaxed);
                Folder(
                    std::env::temp_dir()
                        .join(format!("xirius-import-shots-{}-{n}", std::process::id())),
                )
            })
            .0
            .clone()
    })
}

/// `secs` of sound as `name`: `shape` gives the level at each second.
fn wav(path: &Path, rate: u32, secs: f64, hz: f32, shape: impl Fn(f64) -> f32) {
    let n = (f64::from(rate) * secs) as usize;
    let x: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            (t as f32 * hz * std::f32::consts::TAU).sin() * shape(t)
        })
        .collect();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(path));
    let _ = xirius_session::export::write_wav(path, &x, &x, rate, 16);
}

/// A MIDI file of the chords `roots`, a bar each at 480 a quarter.
fn mid(path: &Path, roots: &[u8]) {
    let mut body = Vec::new();
    for &root in roots {
        for (k, step) in [0u8, 4, 7].into_iter().enumerate() {
            body.extend([0x00, 0x90, root + step, 90 - 10 * k as u8]);
        }
        // 1920 ticks, as a variable-length quantity.
        body.extend([0x8f, 0x00, 0x80, root, 0]);
        body.extend([0x00, 0x80, root + 4, 0, 0x00, 0x80, root + 7, 0]);
    }
    body.extend([0x00, 0xff, 0x2f, 0x00]);
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend([0, 0, 0, 1, 0x01, 0xe0]);
    out.extend(b"MTrk");
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    let _ = std::fs::write(path, out);
}

/// The place's files: folders of drums and loops, a pad at 44.1 kHz, two MIDI files, and
/// what the browser leaves out (a text file, a folder whose name starts with a dot).
pub fn make_place(top: &Path) {
    let _ = std::fs::remove_dir_all(top);
    let hit = |d: f64| move |t: f64| (0.9 * (-t / d).exp()) as f32;
    wav(&top.join("Drums/kick_01.wav"), 48_000, 0.5, 55.0, hit(0.12));
    wav(&top.join("Drums/kick_02.wav"), 48_000, 0.6, 48.0, hit(0.18));
    wav(
        &top.join("Drums/snare_01.wav"),
        48_000,
        0.4,
        210.0,
        hit(0.08),
    );
    wav(
        &top.join("Drums/hat_closed.wav"),
        48_000,
        0.2,
        900.0,
        hit(0.03),
    );
    // Four beats a second at 120 BPM, louder on the first.
    let beats = |bpm: f64| {
        move |t: f64| {
            let beat = t * bpm / 60.0;
            let k = if beat as i64 % 4 == 0 { 0.8 } else { 0.45 };
            (k * (-(beat.fract()) * 6.0).exp()) as f32
        }
    };
    wav(
        &top.join("Loops/loop_120bpm.wav"),
        48_000,
        4.0,
        82.0,
        beats(120.0),
    );
    wav(
        &top.join("Loops/loop_96bpm.wav"),
        48_000,
        5.0,
        98.0,
        beats(96.0),
    );
    wav(&top.join("pad_44k.wav"), 44_100, 6.0, 220.0, |t| {
        (0.6 * (t / 3.0 * std::f64::consts::PI).sin()) as f32
    });
    mid(&top.join("chords.mid"), &[60, 65, 67, 60]);
    mid(&top.join("idea.mid"), &[57, 62]);
    let _ = std::fs::write(top.join("notes.txt"), "見本の素材");
    let _ = std::fs::create_dir_all(top.join(".cache"));
}

/// The place's folder.
pub fn place() -> PathBuf {
    dir().join(PLACE)
}

/// Where the saved song keeps its copy of a file read from the place: the song copies what it
/// reads (BRW-004), so its clips name the copy.
pub fn imported(s: &Studio, name: &str) -> PathBuf {
    let dir = s.song().dir().expect("saved");
    s.project()
        .media
        .place(&dir, xirius_core::model::Media::IMPORTS)
        .join(name)
}

/// Turns the screen's poll until `done` or `secs`.
fn wait(s: &mut Studio, secs: f32, done: impl Fn(&Studio) -> bool) {
    let end = Instant::now() + Duration::from_secs_f32(secs);
    while Instant::now() < end && !done(s) {
        s.idle();
        s.tick();
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// The peaks of `path` are there to draw.
fn drawn(path: &Path, store: &Path) -> bool {
    matches!(wave::look(path, store), Look::Ready(_))
}

/// Where the browser shows the file row `name`, as a hit.
pub fn file_row(s: &Studio, g: &Geometry, name: &str) -> Option<usize> {
    #[cfg(test)]
    crate::browser_files::settle_rows(&s.browser);
    let l = s.browser.layout(g.panel);
    l.rows
        .iter()
        .position(|p| matches!(&p.row, BRow::File { name: n, .. } if n == name))
}

/// A point on the lanes over an audio track's row.
pub fn over_audio(s: &Studio, g: &Geometry) -> Option<(f32, f32)> {
    let x = g.lanes.x + g.lanes.w * 0.35;
    let mut y = g.lanes.y + 4.0;
    while y < g.lanes.y + g.lanes.h {
        if s.audio_track_at(g, x, y).is_some() {
            return Some((x, y + 8.0));
        }
        y += 4.0;
    }
    None
}

/// Scrolls the arrangement until the first audio track's row is wholly on the lanes.
fn show_audio(s: &mut Studio, g: &Geometry) {
    let p = s.project();
    let Some((y, h)) = s
        .tree()
        .rows
        .iter()
        .find(|r| {
            matches!(r.kind, crate::tree::RowKind::Track(t)
                if p.tracks.get(&t).is_some_and(|t| t.kind == TrackKind::Audio))
        })
        .map(|r| (r.y, r.h))
    else {
        return;
    };
    if y < s.scroll.1 || y + h > s.scroll.1 + g.lanes.h {
        s.scroll.1 = (y - 40.0).max(0.0);
        // As the screen holds it, so the point found is where the screen shows the track.
        s.clamp_scroll(g);
    }
}

/// Plays stage `stage` of [`IMPORT`] (stage 0 also comes in the main script).
pub fn stage(s: &mut Studio, g: &Geometry, stage: u8) {
    match stage {
        // The place and a scratch store for the peaks of files outside the song.
        0 => {
            let top = place();
            make_place(&top);
            s.browser.peak_store = Some(dir().join("Peaks"));
            if let Err(e) = s.browser.add_place(&top) {
                s.error = Some(e);
            }
        }
        // The song saved beside it, so its clips' peaks go in its folder.
        1 => {
            let song = dir().join("曲");
            s.command(|x| x.save_as(&song));
            s.follow_browser();
        }
        2 => {
            let Some(key) = s.browser.playing.clone() else {
                return;
            };
            wait(s, 10.0, |_| crate::audition::drawn(&key));
            wait(s, 1.2, |s| {
                s.browser
                    .playing
                    .as_deref()
                    .and_then(crate::audition::progress)
                    .is_some_and(|p| p > 0.4)
            });
        }
        3 => {
            s.browser.search = "kick".into();
            s.browser.scroll = 0.0;
        }
        4 => {
            s.browser.search.clear();
            s.browser.playing = None;
            s.idle();
            show_audio(s, g);
            let (Some(i), Some((x, y))) = (file_row(s, g, "loop_120bpm.wav"), over_audio(s, g))
            else {
                s.error = Some("ドラッグする行か置く先がありません".into());
                return;
            };
            let l = s.browser.layout(g.panel);
            let Some((fx, fy)) = s.browser.center_of(&l, BrowserHit::File(i)) else {
                return;
            };
            s.file_grab(g, Hit::Browser(BrowserHit::File(i)), fx, fy, 1);
            s.file_move((fx + x) / 2.0, (fy + y) / 2.0);
            s.file_move(x, y);
        }
        5 => {
            let Some((x, y)) = s.browser.file_drag.as_ref().map(|d| d.at) else {
                return;
            };
            s.file_up(g, x, y);
            let path = imported(s, "loop_120bpm.wav");
            let store = peaks::store_dir(s.song().dir().as_deref());
            wait(s, 10.0, |_| drawn(&path, &store));
        }
        6 => {
            let files = [place().join("pad_44k.wav"), place().join("chords.mid")];
            s.finish_picked(FileRequest::Audio, &files);
            // Down to the tracks they are on, the last ones.
            s.scroll.1 = f32::MAX;
            s.clamp_scroll(g);
            let pad = imported(s, "pad_44k.wav");
            let store = peaks::store_dir(s.song().dir().as_deref());
            wait(s, 10.0, |_| drawn(&pad, &store));
        }
        7 => s.finish_picked(FileRequest::Place, &[place().join("Drums")]),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
