//! `--script plugin`: the plugin windows, opened as a person opens them, by pointer events on
//! the window (`Act::Press`): the keys button at the head of an instrument track and the
//! plugin's row in the mixer, for a built-in and a VST3 instrument and effect, and for a VST3
//! that runs in a plugin host process.
//!
//! The VST3s are this PC's (kHs Gain; sforzando, else Vital): a stage says `plugin-skip` when
//! one is missing. Each stage writes what it saw to stderr as `plugin-check <shot> …` lines,
//! which `tests/plugin_windows.rs` reads: where a built-in's panel is in the shot, and the
//! picture of each VST3 window, taken next to the shot as `<shot>-editor.bmp`.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{App, Window};
use serde_json::json;
use xirius_core::id::TrackId;
use xirius_design::Layout;
use xirius_plugins::{Catalog, Entry};

use crate::instrument::{DEFAULT, Pick, current};
use crate::mixer::editors::Where;
use crate::shot::{Act, Part, Shot, Target};
use crate::studio::{DrawerTab, Studio};
use crate::tree::RowKind;

use Act::{Plugin, Press, Reset};
use Target::{Keys, Strip, Tab};

pub const SHOTS: &[Shot] = &[
    // The song as it starts: Keys has no instrument, so its list opens.
    Shot {
        name: "p01-keys-without-instrument",
        acts: &[Reset, Plugin(0), Press(Keys("Keys")), Plugin(9)],
    },
    // The built-in instrument's window, from the keys button.
    Shot {
        name: "p02-keys-built-in",
        acts: &[Reset, Plugin(0), Plugin(1), Press(Keys("Keys")), Plugin(9)],
    },
    // A VST3 instrument's window, from the keys button; its picture on the next shot.
    Shot {
        name: "p03-keys-vst3",
        acts: &[
            Reset,
            Plugin(3),
            Plugin(0),
            Plugin(2),
            Press(Keys("Keys")),
            Plugin(9),
        ],
    },
    Shot {
        name: "p04-keys-vst3-window",
        acts: &[Plugin(8), Plugin(9)],
    },
    // A built-in effect's window, from its row in the mixer.
    Shot {
        name: "p05-mixer-built-in-effect",
        acts: &[
            Reset,
            Plugin(4),
            Press(Tab(DrawerTab::Mixer)),
            Press(Strip(Part::Insert, "Guitar")),
            Plugin(9),
        ],
    },
    // A VST3 effect's window, from its row in the mixer.
    Shot {
        name: "p06-mixer-vst3-effect",
        acts: &[
            Reset,
            Plugin(3),
            Plugin(5),
            Press(Tab(DrawerTab::Mixer)),
            Press(Strip(Part::Insert, "Vocal")),
            Plugin(9),
        ],
    },
    Shot {
        name: "p07-mixer-vst3-effect-window",
        acts: &[Plugin(8), Plugin(9)],
    },
    // A VST3 instrument's window, from its row in the mixer.
    Shot {
        name: "p08-mixer-vst3-instrument",
        acts: &[
            Reset,
            Plugin(3),
            Plugin(2),
            Press(Tab(DrawerTab::Mixer)),
            Press(Strip(Part::Insert, "Keys")),
            Plugin(9),
        ],
    },
    Shot {
        name: "p09-mixer-vst3-instrument-window",
        acts: &[Plugin(8), Plugin(9)],
    },
    // A VST3 effect in a plugin host process of its own: the host opens the window.
    Shot {
        name: "p10-host-vst3-effect",
        acts: &[
            Reset,
            Plugin(3),
            Plugin(5),
            Plugin(6),
            Press(Tab(DrawerTab::Mixer)),
            Press(Strip(Part::Insert, "Vocal")),
            Plugin(9),
        ],
    },
    Shot {
        name: "p11-host-vst3-effect-window",
        acts: &[Plugin(8), Plugin(9)],
    },
];

/// The VST3 effect the run opens.
const EFFECT: &str = "Kilohearts/kHs Gain.vst3";
/// The VST3 instruments it tries, in order; `XIRIUS_TEST_VST3_INSTRUMENT` names another
/// bundle in the standard folder.
const INSTRUMENTS: [&str; 2] = ["sforzando.vst3", "Vital.vst3"];
/// How long a plugin host process may take to show its window.
const HOST_WAIT: Duration = Duration::from_secs(5);

fn track_named(s: &Studio, name: &str) -> Option<TrackId> {
    let p = s.project();
    p.tracks.values().find(|t| t.name == name).map(|t| t.id)
}

fn probe(bundle: &str) -> Option<Vec<Entry>> {
    let path = xirius_vst3::default_vst3_dir().join(bundle);
    if !path.exists() {
        return None;
    }
    xirius_plugins::probe(&path).ok()
}

/// This PC's test plugins, as the insert list and the player's list.
fn use_test_plugins(s: &mut Studio, shot: &str) {
    let mut entries = Vec::new();
    match probe(EFFECT) {
        Some(e) => entries.extend(e),
        None => eprintln!("plugin-skip {shot} effect: {EFFECT} is not on this PC"),
    }
    let wanted = std::env::var("XIRIUS_TEST_VST3_INSTRUMENT").ok();
    let names: Vec<&str> = match &wanted {
        Some(n) => vec![n.as_str()],
        None => INSTRUMENTS.to_vec(),
    };
    match names.iter().find_map(|n| probe(n)) {
        Some(e) => entries.extend(e),
        None => eprintln!(
            "plugin-skip {shot} instrument: none of {} is on this PC",
            names.join(", ")
        ),
    }
    s.use_plugins(Catalog {
        entries,
        failures: Vec::new(),
    });
}

/// The first VST3 instrument of the list in use.
pub fn instrument_pick(s: &mut Studio) -> Option<Pick> {
    let catalog = s.plugin_catalog();
    catalog
        .entries
        .iter()
        .find(|e| e.kind == xirius_core::model::PluginKind::Instrument && e.format == "vst3")
        .map(|e| Pick {
            name: e.name.clone(),
            format: e.format.clone(),
            uid: e.uid.clone(),
        })
}

fn add_effect(s: &mut Studio, track: &str, name: &str, format: &str, uid: &str) {
    let Some(t) = track_named(s, track) else {
        eprintln!("plugin: no {track} track");
        return;
    };
    if !s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": name, "kind": "effect",
                "format": format, "uid": uid }),
    ) {
        eprintln!("plugin: could not add {name} to {track}: {:?}", s.error);
    }
}

/// Takes the picture of each open VST3 window next to the shot.
fn capture_windows(s: &mut Studio, shot: &str, dir: &Path) {
    if s.mixer.editors.is_empty() {
        eprintln!("plugin-check {shot} editors=0");
    }
    let open: Vec<_> = s
        .mixer
        .editors
        .iter()
        .map(|e| {
            let app = match &e.at {
                Where::Local { editor, .. } | Where::Apart { editor, .. } => {
                    Some((editor.hwnd(), editor.took_foreground()))
                }
                Where::Host { .. } => None,
            };
            (app, e.title.clone(), e.heard())
        })
        .collect();
    for (app, title, heard) in open {
        let (hwnd, place, front) = match app {
            Some((hwnd, front)) => (Some(hwnd), "app", front),
            None => {
                let t = Instant::now();
                let mut hwnd = None;
                while hwnd.is_none() && t.elapsed() < HOST_WAIT {
                    hwnd = crate::park::any_window(&title);
                    if hwnd.is_none() {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                (hwnd, "host", false)
            }
        };
        let Some(hwnd) = hwnd else {
            eprintln!("plugin-check {shot} editor=missing place={place} title={title}");
            continue;
        };
        let path = dir.join(format!("{shot}-editor.bmp"));
        let foreground = front || crate::park::is_foreground(hwnd);
        // Off every screen: right of the desktop, where the run puts plugin windows.
        let edge = xirius_vst3::Placement::offscreen().at.map_or(0, |(x, _)| x);
        let offscreen = crate::park::origin(hwnd).is_some_and(|(x, _)| x >= edge);
        // The plugin draws at its own pace: in the host's process, or on this thread once its
        // window's messages are let through (only its own: the app's come back on the loop).
        let t = Instant::now();
        let mut taken = xirius_vst3::capture_window(hwnd, &path);
        while t.elapsed() < HOST_WAIT && taken.as_ref().is_ok_and(|c| c.distinct_colors <= 1) {
            let own = s
                .mixer
                .editors
                .iter_mut()
                .filter_map(|e| e.editor_mut())
                .find(|e| e.hwnd() == hwnd);
            match own {
                Some(editor) => editor.pump_own(Duration::from_millis(200)),
                None => std::thread::sleep(Duration::from_millis(200)),
            }
            taken = xirius_vst3::capture_window(hwnd, &path);
        }
        match taken {
            Ok(c) => eprintln!(
                "plugin-check {shot} editor={} place={place} heard={heard} foreground={foreground} offscreen={offscreen} width={} height={} colors={} title={title}",
                path.display(),
                c.width,
                c.height,
                c.distinct_colors,
            ),
            Err(err) => eprintln!("plugin-check {shot} editor=failed place={place} error={err}"),
        }
    }
}

/// Says what is open, and where each built-in's panel is in the shot (physical pixels).
fn report(s: &Studio, shot: &str, window: &Window) {
    let g = crate::screen::geometry(s, window);
    let scale = window.scale_factor();
    for (i, w) in s.instruments.open.iter().enumerate() {
        let r = s.instruments.rect(i, g.size);
        eprintln!(
            "plugin-check {shot} panel={} x={} y={} w={} h={}",
            w.plugin,
            (r.x * scale).round(),
            ((r.y + Layout::TOP_BAR_H) * scale).round(),
            (r.w * scale).round(),
            (r.h * scale).round(),
        );
    }
    eprintln!(
        "plugin-check {shot} popup={:?} built-in={} vst3={} error={:?}",
        s.popup.as_ref().map(|p| p.kind),
        s.instruments.open.len(),
        s.mixer.open_editors(),
        s.error,
    );
}

/// Plays stage `stage` of the shot `shot`, whose pictures go to `dir`.
pub fn play(
    stage: u8,
    studio: &std::rc::Rc<std::cell::RefCell<Studio>>,
    shot: Option<(&Path, &str)>,
    window: &mut Window,
    _: &mut App,
) {
    let (dir, name) = shot.unwrap_or((Path::new("."), "plugin"));
    let mut s = studio.borrow_mut();
    let g = crate::screen::geometry(&s, window);
    let Some(t) = track_named(&s, "Keys") else {
        eprintln!("plugin: no Keys track");
        return;
    };
    match stage {
        // Keys' row in sight above the drawer, as a person would scroll it.
        0 => {
            if let Some(row) = s.tree().row_of(RowKind::Track(t)).cloned() {
                let bottom = s.row_y(&g, &row) + row.h;
                let over = bottom - (g.column.y + g.column.h);
                if over > 0.0 {
                    s.scroll.1 += over;
                    s.clamp_scroll(&g);
                }
            }
        }
        1 => {
            s.set_instrument(t, &Pick::built_in(DEFAULT));
        }
        2 => match instrument_pick(&mut s) {
            Some(pick) => {
                s.set_instrument(t, &pick);
            }
            None => eprintln!("plugin-skip {name} instrument: no VST3 instrument"),
        },
        3 => use_test_plugins(&mut s, name),
        4 => add_effect(&mut s, "Guitar", "EQ", "builtin", "eq"),
        5 => {
            let effect = s
                .plugin_catalog()
                .entries
                .iter()
                .find(|e| e.kind == xirius_core::model::PluginKind::Effect)
                .cloned();
            match effect {
                Some(e) => add_effect(&mut s, "Vocal", &e.name, &e.format, &e.uid),
                None => eprintln!("plugin-skip {name} effect: no VST3 effect"),
            }
        }
        // Vocal's effect in a plugin host process of its own.
        6 => {
            if xirius_engine::server::find_host().is_none() {
                eprintln!("plugin-skip {name} host: xirius-plugin-host is not built");
            }
            let id = track_named(&s, "Vocal")
                .and_then(|v| s.project().tracks[&v].inserts.first().map(|x| x.id));
            if let Some(id) = id
                && !s.run_command(
                    "plugin.isolation.set",
                    json!({ "ids": [id.to_string()], "mode": "dedicated" }),
                )
            {
                eprintln!(
                    "plugin: could not move it to its own process: {:?}",
                    s.error
                );
            }
        }
        8 => capture_windows(&mut s, name, dir),
        _ => {
            let p = s.project();
            let open = p
                .tracks
                .get(&t)
                .and_then(|x| current(x))
                .map(|x| s.mixer.is_open(x.id) || s.instruments.is_open(x.id));
            eprintln!("plugin-check {name} keys-open={open:?}");
            report(&s, name, window);
        }
    }
    window.refresh();
}
