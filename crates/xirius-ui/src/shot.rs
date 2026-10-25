//! The screenshot run: a fixed list of things a person would do, and a way to
//! take a picture of the window after each without touching the person's PC.
//!
//! The window is opened without focus and parked outside every display
//! (`park.rs`); the input is built here and sent through
//! `Window::dispatch_event`, never as OS key input. The picture comes from
//! `PrintWindow` on the window's own handle, so what is in front of it does
//! not matter.
//!
//! The list names its targets ("the M button of Drums"); [`resolve`] turns a
//! name into a [`Hit`] against the current song, so the list does not depend
//! on ids and is tested without a window.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{App, Window};
use xirius_core::model::Project;

use crate::browser::{BrowserHit, RailTab};
use crate::mixer::MixerHit;
use crate::studio::{DrawerTab, Geometry, Hit, Studio};
use crate::tools::Tool;
use crate::topbar::BarButton;
use crate::tree::RowKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// The chevron of the folder with this name.
    Chevron(&'static str),
    Mute(&'static str),
    Solo(&'static str),
    /// A track's row (its name), by track name.
    Track(&'static str),
    /// A folder's row, by folder name.
    Folder(&'static str),
    /// The first clip of the track with this name.
    Clip(&'static str),
    Tab(DrawerTab),
    DrawerToggle,
    /// The ruler at the start of a bar (1-based).
    Bar(u32),
    VisibleRuler,
    Top(BarButton),
    Rail(RailTab),
    Browser(BrowserHit),
    Color(&'static str),
    Widen(&'static str),
    Output(&'static str),
    PopupText(&'static str),
    PopupIcon(&'static str),
    /// The pop-over's first cell whose balloon starts with this.
    PopupTip(&'static str),
    Swatch(usize),
    ArrangeToggle,
    TempoToggle,
    AddTrack,
    /// A part of the mixer strip of the track with this name.
    Strip(Part, &'static str),
    /// The keys button at the head of the instrument track with this name.
    Keys(&'static str),
}

/// The parts of a mixer strip the run points at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    Fader,
    Pan,
    Output,
    AddInsert,
    AddSend,
    /// The first insert row.
    Insert,
}

#[derive(Clone, Copy, Debug)]
pub enum Act {
    Hover(Target),
    Press(Target),
    /// A wheel turn with the pointer on the target.
    Wheel {
        at: Target,
        dx: f32,
        dy: f32,
        ctrl: bool,
    },
    /// Press on the target, move by (dx, dy) with the button held, let go.
    Drag {
        at: Target,
        dx: f32,
        dy: f32,
    },
    /// `ctrl-z`, `ctrl-shift-z`, ...
    Key(&'static str),
    /// Stage `n` of a script: what is not a press or a key, played by the script's own file.
    Stage(Play, u8),
    // The scripts below still have a variant of their own, from before `Stage`; each moves to
    // `Stage` when its file is next changed, and no new script adds one.
    /// A stage of [`crate::rollruler_shots`]: the piano roll's ruler and loop band.
    RollRuler(u8),
    /// A stage of [`crate::import_shots`]: the file browser on real folders.
    Import(u8),
    /// A stage of [`crate::clipedit_shots`]: fades, gain, crossfade, join and slip.
    ClipEdit(u8),
    /// A stage of [`crate::plugin_shots`]: the plugin windows.
    Plugin(u8),
    /// A stage of [`crate::settings::shots`]: the settings screen.
    Settings(u8),
    Reset,
}

/// How a script plays one of its stages. The script's file holds the function, so a new
/// script adds nothing here.
#[derive(Clone, Copy)]
pub enum Play {
    /// On the song and the screen's state alone.
    Studio(fn(&mut Studio, u8)),
    /// On the screen's state laid out in the window's size.
    Laid(fn(&mut Studio, &Geometry, u8)),
    /// Through the window's own input path.
    Window(fn(u8, &Rc<RefCell<Studio>>, &mut Window, &mut App)),
}

impl Play {
    /// Plays stage `n` without a window; false for a stage that needs one.
    pub fn without_window(self, s: &mut Studio, g: &Geometry, n: u8) -> bool {
        match self {
            Play::Studio(f) => f(s, n),
            Play::Laid(f) => f(s, g, n),
            Play::Window(_) => return false,
        }
        true
    }
}

impl std::fmt::Debug for Play {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Play::Studio(_) => "Studio",
            Play::Laid(_) => "Laid",
            Play::Window(_) => "Window",
        })
    }
}

pub struct Shot {
    pub name: &'static str,
    pub acts: &'static [Act],
}

/// Stage `n` of the piano roll's part of [`SCRIPT`].
const fn roll(n: u8) -> Act {
    Act::Stage(Play::Laid(crate::roll::probe::shot), n)
}

/// Stage `n` of the arrangement's part of [`SCRIPT`]: a drag left in the middle.
const fn arrange(n: u8) -> Act {
    Act::Stage(Play::Laid(crate::arrange::probe::shot), n)
}

/// Stage `n` of [`WIRE`]: recording and exporting.
const fn wire_stage(n: u8) -> Act {
    Act::Stage(Play::Studio(wire), n)
}

use Act::{Drag, Hover, Key, Press, Wheel};
use Target::{Bar, Chevron, Clip, DrawerToggle, Folder, Mute, Tab, Track};

/// What the run does, in order; each shot starts from where the last ended.
pub const SCRIPT: &[Shot] = &[
    Shot {
        name: "01-rest",
        acts: &[],
    },
    // MIX-038: the range line of the folder under the pointer turns accent.
    Shot {
        name: "02-hover-nested-track",
        acts: &[Hover(Track("Conga"))],
    },
    Shot {
        name: "03-select-folder",
        acts: &[Press(Folder("Percussion"))],
    },
    Shot {
        name: "04-mute-drums",
        acts: &[Press(Mute("Drums"))],
    },
    Shot {
        name: "05-undo",
        acts: &[Key("ctrl-z")],
    },
    Shot {
        name: "06-redo",
        acts: &[Key("ctrl-shift-z")],
    },
    Shot {
        name: "07-undo-again",
        acts: &[Key("ctrl-z")],
    },
    Shot {
        name: "08-collapse-percussion",
        acts: &[Press(Chevron("Percussion"))],
    },
    Shot {
        name: "09-collapse-rhythm",
        acts: &[Press(Chevron("Rhythm"))],
    },
    Shot {
        name: "10-expand-all",
        acts: &[Press(Chevron("Rhythm")), Press(Chevron("Percussion"))],
    },
    Shot {
        name: "11-midi-clip-roll",
        // The list goes down a row first: the bottom bar leaves Keys under the fold (UI-300).
        acts: &[
            Wheel {
                at: Track("Bass"),
                dx: 0.0,
                dy: -64.0,
                ctrl: false,
            },
            Press(Clip("Keys")),
        ],
    },
    Shot {
        name: "12-mixer",
        acts: &[Press(Tab(DrawerTab::Mixer))],
    },
    Shot {
        name: "13-drawer-shut",
        acts: &[Press(DrawerToggle)],
    },
    Shot {
        name: "14-drawer-open",
        acts: &[Press(DrawerToggle), Press(Tab(DrawerTab::Roll))],
    },
    Shot {
        name: "15-zoom-in",
        acts: &[Wheel {
            at: Target::VisibleRuler,
            dx: 0.0,
            dy: 120.0,
            ctrl: true,
        }],
    },
    Shot {
        name: "16-scroll-right",
        acts: &[Wheel {
            at: Track("Bass"),
            dx: -400.0,
            dy: 0.0,
            ctrl: false,
        }],
    },
    Shot {
        name: "17-zoom-out",
        acts: &[Wheel {
            at: Target::VisibleRuler,
            dx: 0.0,
            dy: -600.0,
            ctrl: true,
        }],
    },
    Shot {
        name: "18-playhead",
        acts: &[Press(Bar(6))],
    },
    Shot {
        name: "19-song-menu",
        acts: &[Act::Reset, Press(Target::Top(BarButton::SongName))],
    },
    Shot {
        name: "20-versions",
        acts: &[Press(Target::PopupText("バージョン一覧…"))],
    },
    Shot {
        name: "21-history",
        acts: &[
            Key("escape"),
            Press(Mute("Drums")),
            Press(Target::Top(BarButton::History)),
        ],
    },
    Shot {
        name: "22-history-undo",
        acts: &[Press(Target::PopupIcon("undo"))],
    },
    Shot {
        name: "23-grid",
        acts: &[Press(Target::Top(BarButton::Grid))],
    },
    Shot {
        name: "24-grid-triplet",
        acts: &[
            Press(Target::PopupText("1/8")),
            Press(Target::PopupText("3連")),
        ],
    },
    Shot {
        name: "25-color",
        acts: &[Key("escape"), Press(Target::Color("Drums"))],
    },
    Shot {
        name: "26-colored-track",
        acts: &[Press(Target::Swatch(4))],
    },
    Shot {
        name: "27-output",
        acts: &[
            Press(Target::Widen("Drums")),
            Press(Target::Output("Drums")),
        ],
    },
    Shot {
        name: "28-browser-views",
        acts: &[
            Key("escape"),
            Press(Target::Browser(BrowserHit::ViewButton)),
        ],
    },
    Shot {
        name: "29-browser-vendor",
        acts: &[Press(Target::PopupText("ベンダー別"))],
    },
    Shot {
        name: "30-effects",
        acts: &[Press(Target::Rail(RailTab::Effects))],
    },
    Shot {
        name: "31-files",
        acts: &[Act::Import(0), Press(Target::Rail(RailTab::Files))],
    },
    Shot {
        name: "32-files-tree",
        acts: &[Press(Target::Browser(BrowserHit::Dir(0)))],
    },
    Shot {
        name: "33-file-audition-row",
        acts: &[Press(Target::Browser(BrowserHit::File(1)))],
    },
    Shot {
        name: "34-rows-hidden",
        acts: &[Press(Target::ArrangeToggle), Press(Target::TempoToggle)],
    },
    Shot {
        name: "35-roll-move",
        acts: &[Act::Reset, roll(0)],
    },
    Shot {
        name: "36-roll-copy",
        acts: &[roll(1)],
    },
    Shot {
        name: "37-roll-lanes",
        acts: &[roll(2)],
    },
    Shot {
        name: "38-roll-add",
        acts: &[roll(3)],
    },
    Shot {
        name: "39-roll-folded",
        acts: &[roll(4)],
    },
    Shot {
        name: "40-roll-menu",
        acts: &[roll(5)],
    },
    // The top bar's tool and grid are the roll's: one state for both.
    Shot {
        name: "41-merged-roll",
        acts: &[Act::Reset, roll(6)],
    },
    Shot {
        name: "42-merged-tool",
        acts: &[Press(Target::Top(BarButton::Tool(Tool::Cut)))],
    },
    Shot {
        name: "43-merged-tool-key",
        acts: &[Key("5")],
    },
    Shot {
        name: "44-merged-grid",
        acts: &[
            Press(Target::Top(BarButton::Grid)),
            Press(Target::PopupText("1/4")),
            Press(Target::PopupText("3連")),
        ],
    },
    // The transport and the mixer, playing to a silent output.
    Shot {
        name: "45-mixer",
        acts: &[Act::Reset, Press(Tab(DrawerTab::Mixer))],
    },
    Shot {
        name: "46-playing",
        acts: &[Press(Bar(2)), Press(Target::Top(BarButton::Play))],
    },
    Shot {
        name: "47-fader-drag",
        acts: &[Drag {
            at: Target::Strip(Part::Fader, "Keys"),
            dx: 0.0,
            dy: 40.0,
        }],
    },
    Shot {
        name: "48-pan-drag",
        acts: &[Drag {
            at: Target::Strip(Part::Pan, "Keys"),
            dx: 0.0,
            dy: -30.0,
        }],
    },
    Shot {
        name: "49-loop",
        acts: &[Press(Target::Top(BarButton::Loop))],
    },
    Shot {
        name: "50-output-list",
        acts: &[Press(Target::Strip(Part::Output, "Keys"))],
    },
    Shot {
        name: "51-insert-list",
        acts: &[
            Key("escape"),
            Press(Target::Strip(Part::AddInsert, "Vocal")),
        ],
    },
    Shot {
        name: "52-send-list",
        acts: &[Key("escape"), Press(Target::Strip(Part::AddSend, "Keys"))],
    },
    Shot {
        name: "53-stopped",
        acts: &[Key("escape"), Press(Target::Top(BarButton::Stop))],
    },
    // EDT-034: a clip dragged later, its start in the balloon.
    Shot {
        name: "54-clip-drag",
        acts: &[Act::Reset, arrange(0)],
    },
    // EDT-035: Alt + drag copies.
    Shot {
        name: "55-clip-alt-copy",
        acts: &[arrange(1)],
    },
    // ARR-001: the loop's end dragged.
    Shot {
        name: "56-loop-end-drag",
        acts: &[arrange(2)],
    },
    // MIX-033: the line where the rows will go.
    Shot {
        name: "57-track-drop-line",
        acts: &[Act::Reset, arrange(3)],
    },
    // MIX-033, MIX-036: into a folder, which lights up.
    Shot {
        name: "58-track-drop-into-folder",
        acts: &[arrange(4)],
    },
    // AUT-004, AUT-005, AUT-014: two lanes, a point dragged with its value shown.
    Shot {
        name: "59-two-lanes-point-drag",
        acts: &[Act::Reset, arrange(5)],
    },
    // AUT-009: the line between two points bent.
    Shot {
        name: "60-lane-curve",
        acts: &[arrange(6)],
    },
    // ARR-004: a section dragged to another place.
    Shot {
        name: "61-section-drag",
        acts: &[Act::Reset, arrange(7)],
    },
    // The tempo track: a tempo point dragged.
    Shot {
        name: "62-tempo-drag",
        acts: &[arrange(8)],
    },
    // Ctrl + wheel makes the tracks taller.
    Shot {
        name: "63-track-height",
        acts: &[Act::Reset, arrange(9)],
    },
    // AUT-001, MIX-033: playing with the mixer open, the Bass lanes in Read:
    // its fader and pan knob stand where the lanes are at the playhead.
    Shot {
        name: "64-playing-mixer-lanes",
        acts: &[
            Act::Reset,
            arrange(10),
            Press(Tab(DrawerTab::Mixer)),
            Press(Bar(5)),
            Press(Target::Top(BarButton::Play)),
        ],
    },
    // A send to a new bus is one step (MIX-025).
    Shot {
        name: "65-new-bus",
        acts: &[
            Press(Target::Top(BarButton::Stop)),
            Press(Target::Strip(Part::AddSend, "Keys")),
            Press(Target::PopupText("新しいバス")),
        ],
    },
    Shot {
        name: "66-new-bus-undone",
        acts: &[Key("ctrl-z")],
    },
    // The + at the head of the track column (mock v4).
    Shot {
        name: "67-add-track-menu",
        acts: &[Act::Reset, Press(Target::AddTrack)],
    },
];

/// Recording, the metronome, the export panel and the mixer (`--script wire`).
/// The song is saved to a scratch folder first, so it can record and export;
/// the input is the test tone, never a microphone.
pub const WIRE: &[Shot] = &[
    Shot {
        name: "00-base",
        acts: &[wire_stage(0)],
    },
    Shot {
        name: "01-metronome-on",
        acts: &[Press(Target::Top(BarButton::Metronome))],
    },
    Shot {
        name: "02-recording",
        acts: &[
            wire_stage(1),
            Press(Target::Top(BarButton::Record)),
            wire_stage(2),
        ],
    },
    Shot {
        name: "03-recorded",
        acts: &[
            Press(Target::Top(BarButton::Record)),
            Press(Target::Top(BarButton::Stop)),
            Press(Target::Top(BarButton::Metronome)),
        ],
    },
    Shot {
        name: "04-export-open",
        acts: &[Press(Target::Top(BarButton::Export))],
    },
    Shot {
        name: "05-export-progress",
        acts: &[wire_stage(3)],
    },
    Shot {
        name: "06-export-done",
        acts: &[wire_stage(4)],
    },
    Shot {
        name: "07-export-stems",
        acts: &[wire_stage(5)],
    },
    Shot {
        name: "08-mixer",
        acts: &[wire_stage(6), Press(Tab(DrawerTab::Mixer))],
    },
    Shot {
        name: "09-plugin-down",
        acts: &[wire_stage(7), Press(Target::Strip(Part::Insert, "Vocal"))],
    },
];

/// Where [`WIRE`] keeps its song.
pub fn wire_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("xirius-wire-shots")
}

/// Ticks the studio for `secs` or until `done`, as the screen would.
pub(crate) fn run_for(s: &mut Studio, secs: f32, done: impl Fn(&Studio) -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs);
    while std::time::Instant::now() < end && !done(s) {
        s.tick();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

/// The stages of [`WIRE`] that are not a press.
pub fn wire(s: &mut Studio, stage: u8) {
    use crate::export::{Act as Xp, State};
    let exported = |s: &Studio| !s.export.running();
    match stage {
        0 => {
            let dir = wire_dir();
            let _ = std::fs::remove_dir_all(&dir);
            s.command(|song| song.save_as(&dir));
        }
        1 => {
            let p = s.project();
            for name in ["Vocal", "Keys"] {
                if let Some(t) = track_named(&p, name) {
                    s.command(|song| song.toggle_arm(t));
                }
            }
            s.seek(p.time_map().bar_start(3));
        }
        2 => run_for(s, 2.0, |_| false),
        3 => {
            s.export_action(Xp::Rate(96_000));
            s.export_action(Xp::Go);
            // A frame or two in: far enough to see it move.
            run_for(s, 0.4, exported);
        }
        4 => run_for(s, 30.0, exported),
        5 => {
            s.export_action(Xp::Rate(48_000));
            s.export_action(Xp::Mode(true));
            s.export_action(Xp::Go);
            run_for(s, 30.0, exported);
            let failed = s
                .export_view()
                .cards
                .iter()
                .any(|c| c.state == State::Failed);
            if failed {
                eprintln!("an export failed: {:?}", s.export.jobs.first());
            }
        }
        6 => s.popup = None,
        // An effect in the shared process that went down and took it with it.
        7 => {
            let Some(t) = track_named(&s.project(), "Vocal") else {
                return;
            };
            s.run_command(
                "plugin.add",
                serde_json::json!({ "track": t.to_string(), "name": "Echo", "kind": "effect",
                                    "format": "vst3", "uid": "00000000000000000000000000000001" }),
            );
            let Some(id) = s.project().tracks[&t].inserts.first().map(|x| x.id) else {
                return;
            };
            s.run_command(
                "plugin.isolation.set",
                serde_json::json!({ "ids": [id.to_string()], "mode": "shared" }),
            );
            s.health.fake = Some(vec![crate::health::Report {
                id,
                crashed: true,
                suggest: true,
            }]);
            s.poll_plugins_now();
        }
        _ => {}
    }
}

fn track_named(project: &Project, name: &str) -> Option<xirius_core::id::TrackId> {
    project
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
}

fn folder_named(project: &Project, name: &str) -> Option<xirius_core::id::FolderId> {
    project
        .folders
        .values()
        .find(|f| f.name == name)
        .map(|f| f.id)
}

/// What `target` is on this song, or None when the song has no such thing.
pub fn resolve(studio: &Studio, target: Target) -> Option<Hit> {
    let project = studio.project();
    Some(match target {
        Target::Chevron(n) => Hit::Chevron(folder_named(&project, n)?),
        Target::Mute(n) => Hit::Mute(track_named(&project, n)?),
        Target::Solo(n) => Hit::Solo(track_named(&project, n)?),
        Target::Track(n) => Hit::Row(RowKind::Track(track_named(&project, n)?)),
        Target::Folder(n) => Hit::Row(RowKind::Folder(folder_named(&project, n)?)),
        Target::Clip(n) => {
            let t = track_named(&project, n)?;
            Hit::Clip(t, project.tracks.get(&t)?.clips.first()?.id)
        }
        Target::Tab(tab) => Hit::Tab(tab),
        Target::DrawerToggle => Hit::Tab(studio.tab),
        Target::Bar(bar) => Hit::Ruler(project.time_map().bar_start(bar)),
        Target::VisibleRuler => Hit::Ruler(studio.px_to_ticks(studio.scroll.0 + 96.0)),
        Target::Top(b) => Hit::Bar(b),
        Target::Rail(tab) => Hit::Rail(tab),
        Target::Browser(h) => Hit::Browser(h),
        Target::Color(n) => Hit::TrackColor(track_named(&project, n)?),
        Target::Widen(n) => Hit::Widen(track_named(&project, n)?),
        Target::Output(n) => Hit::Output(track_named(&project, n)?),
        Target::Keys(n) => Hit::Keys(track_named(&project, n)?),
        Target::Strip(part, n) => {
            let t = track_named(&project, n)?;
            Hit::Mixer(match part {
                Part::Fader => MixerHit::Fader(t),
                Part::Pan => MixerHit::Pan(t),
                Part::Output => MixerHit::Output(t),
                Part::AddInsert => MixerHit::AddInsert(t),
                Part::AddSend => MixerHit::AddSend(t),
                Part::Insert => MixerHit::Open(t, project.tracks.get(&t)?.inserts.first()?.id),
            })
        }
        Target::ArrangeToggle => Hit::ToggleArrange,
        Target::TempoToggle => Hit::ToggleTempo,
        Target::AddTrack => Hit::AddTrack,
        // Only the cell index is used here. center_of resolves its position
        // with the live window size; resizing never changes the cell order.
        Target::PopupText(text) => Hit::Item(
            studio
                .popup
                .as_ref()?
                .layout((1440.0, 860.0))
                .cells
                .iter()
                .position(|c| c.live() && c.text == text)?,
        ),
        Target::PopupTip(tip) => Hit::Item(
            studio
                .popup
                .as_ref()?
                .layout((1440.0, 860.0))
                .cells
                .iter()
                .position(|c| c.tip.starts_with(tip))?,
        ),
        Target::PopupIcon(icon) => Hit::Item(
            studio
                .popup
                .as_ref()?
                .layout((1440.0, 860.0))
                .cells
                .iter()
                .position(|c| c.live() && c.icon == Some(icon))?,
        ),
        Target::Swatch(i) => Hit::Item(
            studio
                .popup
                .as_ref()?
                .layout((1440.0, 860.0))
                .cells
                .iter()
                .position(|c| c.action == Some(crate::popup::Action::Color(Some(i))))?,
        ),
    })
}

// ----------------------------------------------------------------- capture

/// Progress of one capture, shared with the thread that takes it.
#[derive(Clone, Debug, Default)]
pub struct Capture {
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Saved,
    Failed,
}

impl Capture {
    pub fn outcome(&self) -> Outcome {
        match self.state.load(std::sync::atomic::Ordering::Acquire) {
            0 => Outcome::Pending,
            1 => Outcome::Saved,
            _ => Outcome::Failed,
        }
    }

    /// Takes the picture of the window titled `title` on another thread (the
    /// window procedure runs on this one, which has to keep pumping messages
    /// for `PrintWindow` to return) and writes it to `path` as a BMP.
    pub fn start(title: &str, path: std::path::PathBuf) -> Capture {
        let capture = Capture::default();
        let state = capture.state.clone();
        let title = title.to_string();
        std::thread::spawn(move || {
            let done = match imp::grab(&title) {
                Ok(bmp) => std::fs::write(&path, bmp).map_err(|e| e.to_string()),
                Err(e) => Err(e),
            };
            if let Err(e) = &done {
                eprintln!("capture of {} failed: {e}", path.display());
            }
            state.store(
                if done.is_ok() { 1 } else { 2 },
                std::sync::atomic::Ordering::Release,
            );
        });
        capture
    }
}

/// A BMP file (top-down, 32 bits per pixel) from BGRA rows.
pub fn bmp(width: u32, height: u32, bgra: &[u8]) -> Vec<u8> {
    let image = u32::try_from(bgra.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(54 + bgra.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + image).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(-(height as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&image.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    out.extend_from_slice(bgra);
    out
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    type Handle = *mut c_void;

    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_ppm: i32,
        y_ppm: i32,
        clr_used: u32,
        clr_important: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetClientRect(hwnd: Handle, rect: *mut Rect) -> i32;
        fn GetDC(hwnd: Handle) -> Handle;
        fn ReleaseDC(hwnd: Handle, dc: Handle) -> i32;
        fn PrintWindow(hwnd: Handle, dc: Handle, flags: u32) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(dc: Handle) -> Handle;
        fn CreateCompatibleBitmap(dc: Handle, w: i32, h: i32) -> Handle;
        fn SelectObject(dc: Handle, object: Handle) -> Handle;
        fn GetDIBits(
            dc: Handle,
            bitmap: Handle,
            start: u32,
            lines: u32,
            bits: *mut c_void,
            info: *mut BitmapInfoHeader,
            usage: u32,
        ) -> i32;
        fn DeleteObject(object: Handle) -> i32;
        fn DeleteDC(dc: Handle) -> i32;
    }

    const PW_CLIENTONLY: u32 = 1;
    const PW_RENDERFULLCONTENT: u32 = 2;
    const DIB_RGB_COLORS: u32 = 0;

    pub fn grab(title: &str) -> Result<Vec<u8>, String> {
        // SAFETY: every handle is checked before use and released on every
        // path; the window belongs to this process; the pixel
        // buffer is exactly width * height * 4 bytes, which is what
        // `GetDIBits` is told it may write.
        unsafe {
            let hwnd = crate::park::own_window(title);
            if hwnd.is_null() {
                return Err(format!("no window titled {title}"));
            }
            let mut rect = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetClientRect(hwnd, &mut rect) == 0 {
                return Err("GetClientRect failed".into());
            }
            let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
            if w <= 0 || h <= 0 {
                return Err(format!("window is {w}x{h}"));
            }
            let screen = GetDC(std::ptr::null_mut());
            let dc = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(dc, bitmap);
            let printed = PrintWindow(hwnd, dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT);
            let mut pixels = vec![0u8; w as usize * h as usize * 4];
            let mut info = BitmapInfoHeader {
                size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                width: w,
                height: -h,
                planes: 1,
                bit_count: 32,
                compression: 0,
                size_image: 0,
                x_ppm: 0,
                y_ppm: 0,
                clr_used: 0,
                clr_important: 0,
            };
            // The bitmap must not be selected into a DC while it is read.
            SelectObject(dc, old);
            let lines = GetDIBits(
                dc,
                bitmap,
                0,
                h as u32,
                pixels.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );
            DeleteObject(bitmap);
            DeleteDC(dc);
            ReleaseDC(std::ptr::null_mut(), screen);
            if printed == 0 {
                return Err("PrintWindow returned 0".into());
            }
            if lines == 0 {
                return Err("GetDIBits returned 0".into());
            }
            // PrintWindow leaves the alpha channel at 0 on some paths.
            for px in pixels.as_chunks_mut::<4>().0 {
                px[3] = 255;
            }
            Ok(super::bmp(w as u32, h as u32, &pixels))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn grab(_title: &str) -> Result<Vec<u8>, String> {
        Err("screenshots are taken on Windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use xirius_session::output::OutputMode;

    use super::*;

    #[test]
    fn every_target_of_the_script_exists_in_the_sample_song() {
        let studio = Studio::sample();
        for shot in SCRIPT
            .iter()
            .chain(WIRE)
            .chain(crate::palette::SHOTS)
            .chain(crate::ai::shots::AI)
        {
            for act in shot.acts {
                let target = match act {
                    Hover(t) | Press(t) => *t,
                    Wheel { at, .. } | Drag { at, .. } => *at,
                    Key(_)
                    | Act::Stage(..)
                    | Act::RollRuler(_)
                    | Act::Import(_)
                    | Act::ClipEdit(_)
                    | Act::Plugin(_)
                    | Act::Settings(_)
                    | Act::Reset => {
                        continue;
                    }
                };
                // Pop-overs, and inserts a stage puts in, come later.
                if matches!(
                    target,
                    Target::PopupText(_)
                        | Target::PopupIcon(_)
                        | Target::PopupTip(_)
                        | Target::Swatch(_)
                        | Target::Strip(Part::Insert, _)
                ) {
                    continue;
                }
                assert!(
                    resolve(&studio, target).is_some(),
                    "{}: {target:?}",
                    shot.name
                );
            }
        }
    }

    #[test]
    fn the_script_names_are_unique_and_in_order() {
        for (script, shots) in crate::SCRIPTS {
            assert!(!shots.is_empty(), "{script}");
            let names: Vec<&str> = shots.iter().map(|s| s.name).collect();
            let mut sorted = names.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, names, "{script}");
        }
        // The list of scripts too: a new one goes in its place by name.
        let names: Vec<&str> = crate::SCRIPTS.iter().map(|(name, _)| *name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, names);
    }

    #[test]
    fn every_scripted_action_hits_its_target_at_all_supported_widths() {
        for width in [960.0, 1280.0, 1440.0] {
            let sample = || {
                let mut s = Studio::sample();
                s.transport.mode = OutputMode::Silent { paced: true };
                s
            };
            let mut studio = sample();
            for shot in SCRIPT {
                for act in shot.acts {
                    crate::browser_files::settle_rows(&studio.browser);
                    let g = studio.geometry(
                        width,
                        860.0
                            - xirius_design::Layout::TOP_BAR_H
                            - xirius_design::Layout::BOTTOM_BAR_H,
                    );
                    match *act {
                        Act::Reset => studio = sample(),
                        Act::Stage(play, stage) => {
                            play.without_window(&mut studio, &g, stage);
                        }
                        Act::RollRuler(_)
                        | Act::ClipEdit(_)
                        | Act::Plugin(_)
                        | Act::Settings(_) => {}
                        Act::Import(stage) => crate::import_shots::stage(&mut studio, &g, stage),
                        // The window sends keys through `edit_key`, which also
                        // takes the tool keys.
                        Key(key) => {
                            studio.edit_key(
                                &g,
                                key.rsplit('-').next().unwrap(),
                                crate::tools::Mods {
                                    ctrl: key.contains("ctrl"),
                                    shift: key.contains("shift"),
                                    alt: false,
                                },
                            );
                        }
                        Hover(target)
                        | Press(target)
                        | Wheel { at: target, .. }
                        | Drag { at: target, .. } => {
                            let hit = resolve(&studio, target)
                                .unwrap_or_else(|| panic!("{}: {target:?}", shot.name));
                            let (x, y) = studio.center_of(&g, hit).unwrap_or_else(|| {
                                panic!("{}: {target:?} off screen at {width}", shot.name)
                            });
                            let actual = studio.hit(&g, x, y);
                            match *act {
                                Press(_) => {
                                    studio.press(&g, actual);
                                }
                                Hover(_) => studio.hover = actual,
                                Drag { dy, .. } => {
                                    studio.press(&g, actual);
                                    assert!(studio.drag_begin(&g, actual, y), "{}", shot.name);
                                    studio.drag_move(y + dy, false);
                                    assert!(studio.drag_end(), "{}", shot.name);
                                }
                                Wheel { dx, dy, ctrl, .. } => {
                                    if ctrl {
                                        studio.zoom(&g, x, dy / 60.0);
                                    } else {
                                        studio.scroll_by(&g, dx, dy, false);
                                    }
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                    assert_eq!(studio.error, None, "{} at {width}", shot.name);
                }
            }
        }
    }

    #[test]
    fn a_name_that_is_not_in_the_song_resolves_to_nothing() {
        let studio = Studio::sample();
        assert_eq!(resolve(&studio, Mute("Theremin")), None);
        assert_eq!(resolve(&studio, Target::Chevron("Nope")), None);
    }

    #[test]
    fn bmp_has_the_header_a_viewer_expects() {
        let file = bmp(2, 1, &[0, 0, 0, 255, 255, 255, 255, 255]);
        assert_eq!(&file[..2], b"BM");
        assert_eq!(file.len(), 54 + 8);
        assert_eq!(u32::from_le_bytes(file[2..6].try_into().unwrap()), 62);
        assert_eq!(i32::from_le_bytes(file[18..22].try_into().unwrap()), 2);
        // Negative height: rows run top to bottom.
        assert_eq!(i32::from_le_bytes(file[22..26].try_into().unwrap()), -1);
        assert_eq!(u16::from_le_bytes(file[28..30].try_into().unwrap()), 32);
    }

    #[test]
    fn the_first_steps_can_be_aimed_at_on_the_sample_at_the_default_size() {
        // The run aims with `center_of`; a target it cannot aim at is skipped,
        // which would leave a picture that shows nothing of the step.
        let studio = Studio::sample();
        let g = studio.geometry(1440.0, 860.0);
        for target in [
            Track("Conga"),
            Folder("Percussion"),
            Mute("Drums"),
            Chevron("Rhythm"),
            Clip("Keys"),
            DrawerToggle,
            Bar(9),
        ] {
            let hit = resolve(&studio, target).unwrap();
            assert!(studio.center_of(&g, hit).is_some(), "{target:?}");
        }
    }
}
