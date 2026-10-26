//! `--script bugs2`: the screens the second round of fixes after the first-start walk-through
//! (`ux-v1-start`) changes. A device with no inputs, a PC with no MIDI ports, and the colours
//! of new tracks, the mixer's menus and names, the song menu's settings, and the settings'
//! plugin page. Nothing opens a real device: the devices are simulated and the output is
//! silent.

use crate::mixer::MixerHit;
use crate::popup::Action;
use crate::settings::Section;
use crate::shot::{Act, Part, Play, Shot, Target};
use crate::start::Entry;
use crate::studio::{DrawerTab, Geometry, Hit, Studio};
use crate::topbar::BarButton;
use serde_json::json;
use xirius_engine::devices::{Device, Simulated};
use xirius_engine::record::AudioInput;

struct Quiet;
impl AudioInput for Quiet {
    fn channels(&self) -> usize {
        2
    }
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
    }
}

/// An empty song on a PC whose only audio device (speakers) has no inputs and with no MIDI ports.
fn speakers_only(s: &mut Studio, g: &Geometry) {
    *s = Studio::new(crate::song::Song::empty());
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    let speakers = Device {
        name: "Speakers (Realtek Audio)".into(),
        inputs: Vec::new(),
        outputs: vec!["Out 1".into(), "Out 2".into()],
    };
    let mut io = s.song.io.lock().unwrap_or_else(|e| e.into_inner());
    io.replace(Box::new(Simulated::new(
        vec![speakers],
        Default::default(),
        Box::new(Quiet),
    )));
    drop(io);
    s.poll_io();
    let g = s.geometry(g.size.0, g.size.1);
    s.resized(&g);
}

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "b01-no-input-entry",
        acts: &[stage(0)],
    },
    Shot {
        name: "b02-no-input-settings",
        acts: &[stage(1)],
    },
    Shot {
        name: "b03-no-midi-ports",
        acts: &[stage(2)],
    },
    Shot {
        name: "b04-track-colours",
        acts: &[stage(3)],
    },
    Shot {
        name: "b05-plugin-page",
        acts: &[stage(4)],
    },
    Shot {
        name: "b06-plugin-page-scanning",
        acts: &[stage(5)],
    },
    Shot {
        name: "b07-master-name",
        acts: &[stage(6)],
    },
    Shot {
        name: "b08-insert-hover-name",
        acts: &[stage(7), Act::Hover(Target::Strip(Part::Insert, "Vocal"))],
    },
    Shot {
        name: "b09-plugin-menu",
        acts: &[stage(8)],
    },
    Shot {
        name: "b10-mixer-paste-effect",
        acts: &[stage(9)],
    },
    Shot {
        name: "b11-song-menu",
        acts: &[stage(10)],
    },
    Shot {
        name: "b12-song-settings-form",
        acts: &[stage(11)],
    },
];

/// The scan the settings' plugin page lists: three plugins by name, two bundles that could
/// not be read.
fn a_scan() -> xirius_plugins::Catalog {
    use std::path::PathBuf;
    use xirius_core::model::PluginKind;
    let entry = |name: &str, vendor: &str, format: &str, ara: bool| xirius_plugins::Entry {
        uid: format!("{:0>32}", name.len()),
        name: name.into(),
        vendor: vendor.into(),
        kind: PluginKind::Effect,
        format: format.into(),
        category: "Fx".into(),
        version: "1.0".into(),
        path: PathBuf::from(format!("C:/plugins/{name}.{format}")),
        ara,
        requires_process_main: false,
    };
    let bad = |name: &str, kind| xirius_plugins::Failure {
        path: PathBuf::from(format!("C:/plugins/{name}.vst3")),
        kind,
        error: String::new(),
    };
    xirius_plugins::Catalog {
        entries: vec![
            entry("Zeta Comp", "Test Audio", "vst3", false),
            entry("Alpha Tune", "Pitch Works", "vst3", true),
            entry("Room Verb", "Test Audio", "clap", false),
        ],
        failures: vec![
            bad("Broken Verb", xirius_plugins::FailureKind::Timeout),
            bad("Hung Delay", xirius_plugins::FailureKind::Crashed),
        ],
    }
}

/// The sample song with the mixer open and a compressor on Vocal.
fn mixer_with_effect(s: &mut Studio, g: &Geometry) -> (Geometry, xirius_core::id::PluginId) {
    *s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(g.size.0, g.size.1);
    let vocal = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .map(|t| t.id)
        .expect("a Vocal track");
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": vocal.to_string(), "name": "Compressor", "kind": "effect", "format": "builtin", "uid": "Compressor" }),
    ));
    let id = s.project().tracks[&vocal]
        .inserts
        .iter()
        .find(|p| p.name == "Compressor")
        .expect("added")
        .id;
    (s.geometry(g.size.0, g.size.1), id)
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    if n >= 4 {
        play_more(s, g, n);
        return;
    }
    speakers_only(s, g);
    let g = &s.geometry(g.size.0, g.size.1);
    match n {
        0 => {
            let pressed = s.press(g, Hit::Start(Entry::Audio));
            assert!(pressed);
        }
        1 => s.open_settings(Section::Audio),
        2 => s.open_settings(Section::Midi),
        3 => {
            for (i, kind) in [
                "track.add.audio",
                "track.add.audio",
                "track.add.audio",
                "track.add.audio",
                "track.add.audio",
            ]
            .into_iter()
            .enumerate()
            {
                s.song
                    .run(kind, json!({ "name": format!("Audio {}", i + 1) }))
                    .unwrap();
            }
            s.after_edit();
        }
        _ => {}
    }
    s.poll_io();
    let g = s.geometry(g.size.0, g.size.1);
    s.resized(&g);
}

/// The stages on the sample song (the rest are on an empty song with no inputs).
fn play_more(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        4 | 5 => {
            *s = Studio::sample();
            s.use_plugins(a_scan());
            s.open_settings(Section::Plugins);
            if n == 5 {
                s.pose_scan(12, 48);
            }
        }
        6 => {
            *s = Studio::sample();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
        }
        7..=9 => {
            let (g2, comp) = mixer_with_effect(s, g);
            match n {
                8 => {
                    let vocal = s
                        .project()
                        .tracks
                        .values()
                        .find(|t| t.name == "Vocal")
                        .map(|t| t.id)
                        .expect("a Vocal track");
                    let (x, y) = s
                        .mixer_center(&g2, MixerHit::Open(vocal, comp))
                        .expect("on screen");
                    assert!(s.scene_context(&g2, x, y));
                }
                9 => {
                    s.song
                        .run("edit.copy", json!({ "plugin": comp.to_string() }))
                        .unwrap();
                    let last = s
                        .mixer_strips(&g2)
                        .into_iter()
                        .rfind(|x| !x.track.is_master())
                        .expect("a strip")
                        .rect;
                    assert!(s.scene_context(&g2, last.right() + 15.0, last.y + 60.0));
                }
                _ => {}
            }
        }
        10 | 11 => {
            *s = Studio::sample();
            let g2 = s.geometry(g.size.0, g.size.1);
            assert!(s.press(&g2, Hit::Bar(BarButton::SongName)));
            if n == 11 {
                assert!(s.popup_action(&g2, Action::SongSettings));
            }
        }
        _ => {}
    }
    s.poll_io();
    let g = s.geometry(g.size.0, g.size.1);
    s.resized(&g);
}
