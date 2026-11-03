//! `--script startup`: the first screen and the first launch (UI-350・UI-262・UI-263・PLG-059・
//! LIC-016). The start screen with the recent songs, the first time, after a cancelled open,
//! with no device and its menu; the new song's screen, with no device and on a device without
//! inputs; the test tone on 設定 › オーディオ, the PC keyboard on the MIDI page, the scan on the
//! plugin page, 「Xirius について」, a newer version, and 「更新を確認」 on 一般. Nothing real is
//! opened: the songs are the script's own, the devices are simulated and the output is silent.

use crate::home::Spot;
use crate::settings::Section;
use crate::shot::{Act, Play, Shot};
use crate::song::Song;
use crate::start::Entry;
#[cfg(test)]
use crate::start::Part;
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
        name: "start-01-home",
        acts: &[staged(0)],
    },
    Shot {
        name: "start-02-first",
        acts: &[staged(1)],
    },
    Shot {
        name: "start-03-cancelled",
        acts: &[staged(2)],
    },
    Shot {
        name: "start-04-nodevice",
        acts: &[staged(3)],
    },
    Shot {
        name: "start-05-nodevice-menu",
        acts: &[staged(4)],
    },
    Shot {
        name: "start-06-new",
        acts: &[staged(5)],
    },
    Shot {
        name: "start-07-new-nodevice",
        acts: &[staged(6)],
    },
    Shot {
        name: "start-08-noinputs",
        acts: &[staged(7)],
    },
    Shot {
        name: "start-09-tone",
        acts: &[staged(8)],
    },
    Shot {
        name: "start-10-midi",
        acts: &[staged(9)],
    },
    Shot {
        name: "start-11-scanning",
        acts: &[staged(10)],
    },
    Shot {
        name: "start-12-about",
        acts: &[staged(11)],
    },
    Shot {
        name: "start-13-newer",
        acts: &[staged(12)],
    },
    Shot {
        name: "start-14-update-setting",
        acts: &[staged(13)],
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

/// The start screen on a PC with these devices, and the songs opened before when `songs`.
fn home(s: &mut Studio, g: &Geometry, devices: &[(&str, usize, usize)], songs: bool) {
    empty(s, g, devices);
    if !songs {
        s.keep = crate::keep::Keep::at(dir().join("first"));
    }
    let end = Instant::now() + Duration::from_secs(5);
    while songs && s.keep.listed().len() < 3 && Instant::now() < end {
        s.keep.poll_recent();
        std::thread::sleep(Duration::from_millis(5));
    }
    s.home = Some(crate::home::Home::default());
    s.home_tick();
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
        // The start screen: the buttons, and the recent songs with the sample last.
        0 => home(s, g, RIG, true),
        // The first time: only the sample, no search, 新規 has the keys.
        1 => home(s, g, RIG, false),
        // The second row's opening, cancelled: back on the start screen, the row still chosen.
        2 => {
            home(s, g, RIG, true);
            s.home_key(g, "down", Default::default());
            s.home_key(g, "enter", Default::default());
            s.cancel_opening();
            let end = Instant::now() + Duration::from_secs(15);
            while s.opening.is_some() && Instant::now() < end {
                s.poll_opening();
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        // No audio device: the line under the buttons, and its menu.
        3 => home(s, g, &[], true),
        4 => {
            home(s, g, &[], true);
            s.press(g, Hit::Home(Spot::Device));
        }
        // The new song: only the three entries.
        5 => empty(s, g, RIG),
        6 => empty(s, g, &[]),
        // The speakers chosen: 「オーディオを録音」 lists the devices with inputs.
        7 => {
            empty(s, g, RIG);
            let _ = s
                .song
                .set("audio.set", json!({ "key": "device", "value": "Speakers" }));
            s.poll_io();
            s.press(g, Hit::Start(Entry::Audio));
        }
        // 「テスト音」 on 設定 › オーディオ › 出力: the left first, the meters while it sounds.
        8 => {
            empty(s, g, RIG);
            // The sound is got ready first, as an opened song's is, so the press sounds.
            s.prepare_song();
            let end = Instant::now() + Duration::from_secs(15);
            while s.sound_preparing() && Instant::now() < end {
                s.poll_preparing();
                std::thread::sleep(Duration::from_millis(5));
            }
            s.open_settings(Section::Audio);
            let p = crate::audioset_shots::item(s, g, crate::settings::Do::Tone);
            s.settings_press(g, p.0, p.1);
            let end = Instant::now() + Duration::from_secs(2);
            while Instant::now() < end {
                std::thread::sleep(Duration::from_millis(30));
                s.rig_tick();
                if s.rig_out[0] > 0.0 {
                    break;
                }
            }
        }
        // The MIDI page: the PC keyboard first, lit by a key.
        9 => {
            empty(s, g, RIG);
            s.open_settings(Section::Midi);
            let later = Instant::now() + Duration::from_secs(3600);
            let mut io = s.song.io.lock().unwrap_or_else(|e| e.into_inner());
            io.heard(PC_KEYBOARD, 67, later);
            io.poll(later);
        }
        // The plugin page while the first scan runs.
        10 => {
            empty(s, g, RIG);
            s.pose_scan(12, 48);
            s.mixer.scan.file =
                r"C:\Program Files\Common Files\VST3\Kilohearts\kHs Gain.vst3".to_string();
            s.open_settings(Section::Plugins);
        }
        // 「Xirius について」: the repository under the version.
        11 => {
            empty(s, g, RIG);
            s.open_about(s.about_anchor(g));
        }
        // A newer version: the logo's dot, and its number, 「ダウンロード」 and what changed.
        12 => {
            empty(s, g, RIG);
            s.update.newer = Some(newer());
            s.open_about(s.about_anchor(g));
        }
        // 一般: 「更新を確認」 at the foot.
        13 => {
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
        assert!(s.home_shown());
        let home = s.home.as_ref().unwrap();
        assert_eq!(home.pick.nav().len(), 4, "three songs and the sample");
        assert_eq!(home.pick.selected, 0);
        stage(&mut s, &g, 1);
        assert_eq!(
            s.home.as_ref().unwrap().pick.nav().len(),
            1,
            "only the sample"
        );
        assert_eq!(s.home_layout(&g).unwrap().search, None);
        stage(&mut s, &g, 2);
        assert!(s.home_shown(), "back on the start screen");
        assert_eq!(s.home.as_ref().unwrap().pick.selected, 1);
        assert_eq!(s.song.dir(), None);
        stage(&mut s, &g, 3);
        assert!(s.home_layout(&g).unwrap().no_device.is_some());
        stage(&mut s, &g, 4);
        assert_eq!(
            s.popup.as_ref().map(|p| p.kind),
            Some(crate::popup::PopupKind::NoDevice)
        );
        stage(&mut s, &g, 5);
        assert!(s.start_shown() && !s.home_shown());
        assert_eq!(s.start_layout(&g).no_device, None);
        stage(&mut s, &g, 6);
        assert!(s.start_layout(&g).no_device.is_some());
        assert!(s.center_of(&g, Hit::Entrance(Part::NoDevice)).is_some());
        stage(&mut s, &g, 7);
        assert_eq!(
            s.popup.as_ref().map(|p| p.kind),
            Some(crate::popup::PopupKind::Device(true))
        );
        stage(&mut s, &g, 8);
        assert!(s.rig_tone);
        assert!(s.settings_scene(&g).has_text("L"));
        stage(&mut s, &g, 12);
        assert!(s.bar_view().update);
        assert_eq!(
            s.popup.as_ref().map(|p| p.kind),
            Some(crate::popup::PopupKind::About)
        );
        stage(&mut s, &g, 13);
        assert!(s.settings_scene(&g).has_text("更新を確認"));
        assert_eq!(s.error, None);
        let _ = std::fs::remove_dir_all(dir());
    }
}
