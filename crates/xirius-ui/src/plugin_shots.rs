//! `--script plugin`: the plugin windows, opened as a person opens them, by pointer events on
//! the window (`Act::Press`): the keys button at the head of an instrument track and the
//! plugin's row in the mixer, for a built-in and a VST3 instrument and effect, and for a VST3
//! that runs in a plugin host process.
//!
//! The shipped VST3 fixture is used in both track roles. Each stage logs the window it opened;
//! each editor is captured in its own bitmap beside the main-window shot.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{App, Window};
use serde_json::json;
use xirius_core::id::TrackId;
use xirius_plugins::Catalog;

use crate::instrument::{DEFAULT, Pick, current};
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
    Shot {
        name: "p02-keys-built-in-window",
        acts: &[Plugin(10)],
    },
    // A VST3 instrument's window, from the keys button; its picture on the next shot.
    Shot {
        name: "p03-keys-vst3",
        acts: &[
            Reset,
            Plugin(11),
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
    Shot {
        name: "p05-mixer-built-in-effect-window",
        acts: &[Plugin(10)],
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
            Plugin(11),
            Plugin(2),
            Press(Tab(DrawerTab::Mixer)),
            Press(Strip(Part::Insert, "Keys")),
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
            Press(Strip(Part::Insert, "Vocal")),
            Plugin(9),
        ],
    },
    Shot {
        name: "p11-host-vst3-effect-window",
        acts: &[Plugin(8), Plugin(9)],
    },
];

/// How long a plugin host process may take to show its window.
const HOST_WAIT: Duration = Duration::from_secs(5);

fn track_named(s: &Studio, name: &str) -> Option<TrackId> {
    let p = s.project();
    p.tracks.values().find(|t| t.name == name).map(|t| t.id)
}

fn use_test_plugins(s: &mut Studio, instrument: bool) {
    let fixture = crate::plugin_fixture_shots::fixture_path();
    let mut entries = xirius_plugins::probe(&fixture).expect("scan the VST3 fixture");
    if instrument {
        entries[0].kind = xirius_core::model::PluginKind::Instrument;
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
        .map(|e| (e.id, e.title.clone()))
        .collect();
    for (id, title) in open {
        #[cfg(not(windows))]
        let _ = id;
        let t = Instant::now();
        let mut hwnd = None;
        while hwnd.is_none() && t.elapsed() < HOST_WAIT {
            s.tick();
            hwnd = crate::park::any_window(&title);
            if hwnd.is_none() {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let Some(hwnd) = hwnd else {
            eprintln!("plugin-check {shot} editor=missing place=host title={title}");
            continue;
        };
        let path = dir.join(format!("{shot}-editor.bmp"));
        let foreground = crate::park::is_foreground(hwnd);
        // Off every screen: right of the desktop, where the run puts plugin windows.
        let edge = xirius_vst3::Placement::offscreen().at.map_or(0, |(x, _)| x);
        let offscreen =
            crate::park::origin(hwnd).is_some_and(|(x, y)| x >= edge || x <= -30000 || y <= -30000);
        let in_host = crate::park::own_handle(&title).is_none();
        // The plugin draws at its own pace in the host process.
        let t = Instant::now();
        let mut taken = xirius_vst3::capture_window(hwnd, &path);
        while t.elapsed() < HOST_WAIT && taken.as_ref().is_ok_and(|c| c.distinct_colors <= 1) {
            std::thread::sleep(Duration::from_millis(200));
            taken = xirius_vst3::capture_window(hwnd, &path);
        }
        match taken {
            Ok(c) => eprintln!(
                "plugin-check {shot} editor={} in_host={in_host} foreground={foreground} offscreen={offscreen} width={} height={} colors={} title={title}",
                path.display(),
                c.width,
                c.height,
                c.distinct_colors,
            ),
            Err(err) => eprintln!("plugin-check {shot} editor=failed place=host error={err}"),
        }
        #[cfg(windows)]
        if std::env::var_os("XIRIUS_HEADER_PROBE").is_some()
            && (shot.ends_with("p07-mixer-vst3-effect-window")
                || shot.ends_with("p09-mixer-vst3-instrument-window"))
        {
            probe_header(s, id, shot, dir);
        }
    }
}

/// Clicks the actual native child buttons while the editor is parked outside the desktop.
/// Each result is checked in the song, not only in the host's command queue.
#[cfg(windows)]
fn probe_header(s: &mut Studio, id: xirius_core::id::PluginId, shot: &str, dir: &Path) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BM_CLICK, GetDlgItem, SMTO_ABORTIFHUNG, SendMessageTimeoutW,
    };
    fn click(
        s: &mut Studio,
        id: xirius_core::id::PluginId,
        shot: &str,
        button: i32,
        label: &str,
        done: &dyn Fn(&Studio) -> bool,
    ) -> bool {
        let hwnd = s.mixer.editor_hwnd(id).unwrap_or_default();
        let child = unsafe { GetDlgItem(hwnd as _, button) };
        if child.is_null() {
            eprintln!("header-probe {shot} {label} missing=true");
            return false;
        }
        let mut result = 0;
        let started = Instant::now();
        let before = s
            .project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|p| p.id == id)
            .map(|p| p.runs());
        let sent = unsafe {
            SendMessageTimeoutW(child, BM_CLICK, 0, 0, SMTO_ABORTIFHUNG, 5_000, &mut result)
        };
        let end = started + Duration::from_secs(10);
        while sent != 0 && !done(s) && Instant::now() < end {
            // The scripted shot holds Studio's mutable borrow, so its normal GPUI watcher
            // cannot borrow it. Drive that same watcher path before the transport tick.
            s.poll_editor_bar();
            s.tick();
            std::thread::sleep(Duration::from_millis(5));
        }
        eprintln!(
            "header-probe {shot} {label} hwnd={hwnd} next_hwnd={:?} before={before:?} after={:?} sent={} effective={} elapsed_ms={} blocked={} preparing={} shown={} error={:?}",
            s.mixer.editor_hwnd(id),
            s.project()
                .tracks
                .values()
                .flat_map(|t| &t.inserts)
                .find(|p| p.id == id)
                .map(|p| p.runs()),
            sent != 0,
            done(s),
            started.elapsed().as_millis(),
            s.input_blocked(),
            s.sound_preparing(),
            s.editor_shown(id),
            s.error,
        );
        done(s)
    }
    let track = s.project().tracks.values().find_map(|t| {
        t.inserts
            .iter()
            .find(|p| p.id == id)
            .map(|p| (t.id, p.kind))
    });
    if let Some((track, kind)) = track {
        for (i, runs) in [false, true, false, true].into_iter().enumerate() {
            assert!(
                click(s, id, shot, 1, &format!("power-{i}"), &|s| {
                    s.project().tracks[&track]
                        .inserts
                        .iter()
                        .any(|p| p.id == id && p.runs() == runs)
                }),
                "power did not reach the song"
            );
            if !runs {
                crate::shot::run_for(s, 1.0, |s| {
                    s.mixer.editor_bars.get(&id).is_some_and(|bar| !bar.powered)
                });
                std::thread::sleep(Duration::from_millis(100));
                if let Some(hwnd) = s.mixer.editor_hwnd(id) {
                    let _ = xirius_vst3::capture_window(
                        hwnd,
                        &dir.join(format!("{shot}-power-off.bmp")),
                    );
                }
            }
        }
        let pinned = s.mixer.editor_pins.contains(&id);
        assert!(
            click(s, id, shot, 4, "pin", &|s| {
                s.mixer.editor_pins.contains(&id) != pinned
            }),
            "pin did not reach the window state"
        );
        assert!(
            click(s, id, shot, 6, "channel", &|s| s.channel.is_some()),
            "channel did not open"
        );
        let kind = if kind == xirius_core::model::PluginKind::Instrument {
            "instrument"
        } else {
            "fx"
        };
        for name in ["Header A", "Header B"] {
            assert!(s.run_command(
                "preset.save",
                json!({"track":track,"of":id,"kind":kind,"name":name})
            ));
            crate::shot::run_for(s, 10.0, |s| !s.input_blocked());
        }
        crate::shot::run_for(s, 10.0, |s| {
            s.mixer
                .editor_bars
                .get(&id)
                .is_some_and(|b| b.presets.len() >= 3)
        });
        assert!(s.run_command(
            "preset.load",
            json!({"track":track,"of":id,"preset":"Header A"})
        ));
        crate::shot::run_for(s, 10.0, |s| {
            s.project().tracks[&track]
                .inserts
                .iter()
                .any(|p| p.id == id && p.preset.as_deref() == Some("Header A"))
        });
        assert!(
            click(s, id, shot, 11, "next", &|s| {
                s.project().tracks[&track]
                    .inserts
                    .iter()
                    .any(|p| p.id == id && p.preset.as_deref() != Some("Header A"))
            }),
            "next preset did not load"
        );
        assert!(
            click(s, id, shot, 8, "previous", &|s| {
                s.project().tracks[&track]
                    .inserts
                    .iter()
                    .any(|p| p.id == id && p.preset.as_deref() == Some("Header A"))
            }),
            "previous preset did not load"
        );
    }
    assert!(
        click(s, id, shot, 5, "close", &|s| !s.editor_shown(id)),
        "editor did not close"
    );
}

#[cfg(windows)]
fn capture_builtin(s: &Studio, shot: &str, dir: &Path, main: &Window) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GW_OWNER, GetWindow, IsWindowVisible};
    let [device] = s.instruments.open.as_slice() else {
        panic!("expected one built-in editor for {shot}");
    };
    let title = format!("Xirius · {}", device.plugin);
    let hwnd = crate::park::own_handle(&title).expect("the built-in has its own window");
    let main = crate::device_window::hwnd_of(main);
    let owned = unsafe { GetWindow(hwnd as _, GW_OWNER) as isize == main };
    let visible = unsafe { IsWindowVisible(hwnd as _) != 0 };
    let foreground = crate::park::is_foreground(hwnd);
    let offscreen = crate::park::origin(hwnd).is_some_and(|(x, y)| x <= -30000 || y <= -30000);
    let capture = xirius_vst3::capture_window(hwnd, &dir.join(format!("{shot}-editor.bmp")))
        .expect("capture the built-in editor");
    eprintln!(
        "plugin-check {shot} built_in=true owned={owned} visible={visible} foreground={foreground} offscreen={offscreen} width={} height={} colors={} title={title}",
        capture.width, capture.height, capture.distinct_colors,
    );
}

/// Says what is open in the model.
fn report(s: &Studio, shot: &str) {
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
    crate::shot::run_for(&mut s, 15.0, |s| !s.input_blocked());
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
        3 | 11 => use_test_plugins(&mut s, stage == 11),
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
        #[cfg(windows)]
        10 => capture_builtin(&s, name, dir, window),
        #[cfg(not(windows))]
        10 => (),
        _ => {
            let p = s.project();
            let open = p
                .tracks
                .get(&t)
                .and_then(|x| current(x))
                .map(|x| s.mixer.is_open(x.id) || s.instruments.is_open(x.id));
            eprintln!("plugin-check {name} keys-open={open:?}");
            report(&s, name);
        }
    }
    // Fixture edits may start the player or load an insert asynchronously.
    // The next scripted press is delivered only when that work has settled.
    if matches!(stage, 1 | 2 | 4 | 5 | 6) {
        s.prepare_song();
    }
    crate::shot::run_for(&mut s, 15.0, |s| !s.input_blocked());
    assert!(
        !s.input_blocked(),
        "plugin fixture did not settle: opening={} saving={} sound={} inserts={} error={:?}",
        s.opening.is_some(),
        s.saving.is_some(),
        s.sound_preparing(),
        s.inserts_wait.is_some(),
        s.error,
    );
    window.refresh();
}
