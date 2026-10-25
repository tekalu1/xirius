//! `--script startup`: the first screen and the first launch (UI-260・UI-261・UI-262・UI-263・
//! PLG-059・LIC-016). The empty song with the panel of the devices and the recent songs, a
//! device without inputs, the test tone, the devices' list, no device, the PC keyboard on the
//! MIDI page, the scan on the plugin page, 「Xirius について」, a newer version, and
//! 「更新を確認」 on 一般. Nothing real is opened: the devices are simulated and the output is
//! silent.

use crate::settings::Section;
use crate::shot::{Act, Play, Shot};
use crate::song::Song;
use crate::start::{Entry, Part};
use crate::studio::{Geometry, Hit, Studio};
use serde_json::json;
use std::time::{Duration, Instant};
use xirius_engine::devices::{Device, Latency, Simulated};
use xirius_engine::record::AudioInput;
use xirius_session::audio_settings::PC_KEYBOARD;

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "start-01-rig",
        acts: &[staged(0)],
    },
    Shot {
        name: "start-02-noinputs",
        acts: &[staged(1)],
    },
    Shot {
        name: "start-03-tone",
        acts: &[staged(2)],
    },
    Shot {
        name: "start-04-devmenu",
        acts: &[staged(3)],
    },
    Shot {
        name: "start-05-nodevice",
        acts: &[staged(4)],
    },
    Shot {
        name: "start-06-midi",
        acts: &[staged(5)],
    },
    Shot {
        name: "start-07-scanning",
        acts: &[staged(6)],
    },
    Shot {
        name: "start-08-about",
        acts: &[staged(7)],
    },
    Shot {
        name: "start-09-newer",
        acts: &[staged(8)],
    },
    Shot {
        name: "start-10-update-setting",
        acts: &[staged(9)],
    },
];

/// Where the script keeps its songs.
fn dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("xirius-startup-shots-{}", std::process::id()))
}

/// An input that hears a voice on its first channel and little on its second.
struct Voice;
impl AudioInput for Voice {
    fn channels(&self) -> usize {
        2
    }
    fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.3);
        right.fill(0.02);
    }
}

/// A new empty song on a PC with these devices, each `(name, inputs, outputs)`, a MIDI
/// keyboard, and the songs opened before.
fn empty(s: &mut Studio, g: &Geometry, devices: &[(&str, usize, usize)]) {
    *s = Studio::new(Song::empty());
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    let devices = devices
        .iter()
        .map(|(name, inputs, outputs)| Device {
            name: name.to_string(),
            inputs: (1..=*inputs).map(|i| format!("In {i}")).collect(),
            outputs: (1..=*outputs).map(|i| format!("Out {i}")).collect(),
        })
        .collect();
    let latency = Latency {
        rate: 48_000,
        input: 144,
        output: 144,
        conversion: 38,
    };
    {
        let mut io = s.song.io.lock().unwrap_or_else(|e| e.into_inner());
        io.replace(Box::new(Simulated::new(
            devices,
            vec![(256, latency)].into_iter().collect(),
            Box::new(Voice),
        )));
    }
    s.poll_io();
    {
        let mut io = s.song.io.lock().unwrap_or_else(|e| e.into_inner());
        io.view.ports = vec!["Arturia KeyStep 37".into()];
        io.view.latency.get_or_insert(latency);
    }
    remember(s);
    s.resized(g);
}

/// Three songs saved before, the newest first.
fn remember(s: &mut Studio) {
    let root = dir();
    let _ = std::fs::remove_dir_all(&root);
    let keep = crate::keep::Keep::at(root.join("keep"));
    let now = xirius_core::store::now_ms();
    for (name, ago_ms) in [
        ("2026-10-04", 9 * 86_400_000),
        ("2026-10-09", 4 * 86_400_000),
        ("2026-10-12", 2 * 3_600_000),
    ] {
        let place = root.join("songs").join(name);
        let mut song = Song::empty();
        let _ = song.run("project.rename", json!({ "name": name }));
        let _ = song.run("track.add.audio", json!({ "name": "Vocal" }));
        let _ = song.save_as(&place);
        keep.remember(&place, name, false, now - ago_ms);
    }
    s.keep = keep;
    s.keep.sync(&s.song.project());
}

/// An interface with two inputs, and the speakers.
const RIG: &[(&str, usize, usize)] = &[("Scarlett 2i2 (4th Gen)", 2, 2), ("Speakers", 0, 2)];

/// A newer version than this one.
fn newer() -> xirius_session::update::Release {
    xirius_session::update::Release {
        version: "1.0.1".into(),
        url: "https://github.com/tekalu1/xirius/releases/tag/v1.0.1".into(),
        notes: vec![
            "録音の入力が、選んだ入力でなく既定の入力になる不具合を直しました".into(),
            "新しい曲に替えても、テーマなどの設定が既定に戻らないようにしました".into(),
        ],
        prerelease: false,
        assets: Vec::new(),
    }
}

/// Plays stage `n` of [`SHOTS`].
fn stage(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        // The empty song: the entries, the panel of the devices (the plugins found so far) and
        // the recent songs.
        0 => {
            empty(s, g, RIG);
            s.pose_scan(4, 12);
        }
        // The speakers chosen: 「オーディオを録音」 lists the devices with inputs.
        1 => {
            empty(s, g, RIG);
            let _ = s
                .song
                .set("audio.set", json!({ "key": "device", "value": "Speakers" }));
            s.poll_io();
            s.press(g, Hit::Start(Entry::Audio));
        }
        // 「テスト音」: the left first.
        2 => {
            empty(s, g, RIG);
            s.press(g, Hit::Entrance(Part::Tone));
            std::thread::sleep(Duration::from_millis(150));
            s.rig_tick();
        }
        // The device's button: every device, the one in use checked.
        3 => {
            empty(s, g, RIG);
            s.press(g, Hit::Entrance(Part::Device));
        }
        4 => empty(s, g, &[]),
        // The MIDI page: the PC keyboard first, lit by a key.
        5 => {
            empty(s, g, RIG);
            s.open_settings(Section::Midi);
            let later = Instant::now() + Duration::from_secs(3600);
            let mut io = s.song.io.lock().unwrap_or_else(|e| e.into_inner());
            io.heard(PC_KEYBOARD, 67, later);
            io.poll(later);
        }
        // The plugin page while the first scan runs.
        6 => {
            empty(s, g, RIG);
            s.pose_scan(12, 48);
            s.mixer.scan.file =
                r"C:\Program Files\Common Files\VST3\Kilohearts\kHs Gain.vst3".to_string();
            s.open_settings(Section::Plugins);
        }
        // 「Xirius について」: the repository under the version.
        7 => {
            empty(s, g, RIG);
            s.open_about(s.about_anchor(g));
        }
        // A newer version: the logo's dot, and its number, 「ダウンロード」 and what changed.
        8 => {
            empty(s, g, RIG);
            s.update.newer = Some(newer());
            s.open_about(s.about_anchor(g));
        }
        // 一般: 「更新を確認」 at the foot.
        9 => {
            empty(s, g, RIG);
            s.update.newer = Some(newer());
            s.open_settings(Section::General);
            let end = s.settings_scene(g).scroll_max;
            if let Some(ui) = &mut s.settings {
                ui.scroll = end;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stage shows what it is for, without a window.
    #[test]
    fn each_stage_shows_its_part() {
        let mut s = Studio::sample();
        let g = s.geometry(
            1440.0,
            860.0 - xirius_design::Layout::TOP_BAR_H - xirius_design::Layout::BOTTOM_BAR_H,
        );
        stage(&mut s, &g, 0);
        assert!(s.start_shown());
        s.keep.settle_recent();
        s.keep.poll_recent();
        assert_eq!(s.start_recent().len(), 3);
        assert_eq!(s.rig().device.as_deref(), Some("Scarlett 2i2 (4th Gen)"));
        stage(&mut s, &g, 1);
        assert_eq!(
            s.popup.as_ref().map(|p| p.kind),
            Some(crate::popup::PopupKind::Device(true))
        );
        stage(&mut s, &g, 3);
        assert_eq!(
            s.popup.as_ref().map(|p| p.kind),
            Some(crate::popup::PopupKind::Device(false))
        );
        stage(&mut s, &g, 4);
        assert_eq!(s.rig().device, None);
        stage(&mut s, &g, 5);
        assert_eq!(s.rig().midi[0], (PC_KEYBOARD.to_string(), true));
        stage(&mut s, &g, 8);
        assert!(s.bar_view().update);
        assert_eq!(
            s.popup.as_ref().map(|p| p.kind),
            Some(crate::popup::PopupKind::About)
        );
        stage(&mut s, &g, 9);
        assert!(s.settings_scene(&g).has_text("更新を確認"));
        assert_eq!(s.error, None);
        let _ = std::fs::remove_dir_all(dir());
    }
}
