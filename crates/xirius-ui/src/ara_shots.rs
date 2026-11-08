//! `--script ara`: ARA on the clips (EDT-023, PLG-036, PLG-055, PLG-058; the approved screens of
//! 2026-11-03). 「処理 ›」 with its last row, the list that picks the plugin, the dim row of a
//! clip that cannot take it, and the badge after an attached clip's name in each state, with
//! the menu a press on a stopped plugin's badge opens. The plugin is a stand-in of the shots
//! (no plugin is loaded, and the installed ones are not shown); its state is set by the script.
//! Silent; no OS input.

use serde_json::json;
use xirius_core::id::ClipId;
use xirius_core::model::PluginKind;
use xirius_plugins::{Catalog, Entry};
use xirius_session::ara::{Phase, Report};

use crate::arrange::probe::{clip_mid, track};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};

const UID: &str = "58495249555341524153484f54530001";
const NAME: &str = "Sample ARA";

const fn laid(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    // 処理 › ends with 「ARA プラグインを追加…」 and its key.
    Shot {
        name: "ara01-process",
        acts: &[Act::Reset, laid(0), laid(1)],
    },
    // The list of the ARA plugins, under their makers, opened at the clip's head.
    Shot {
        name: "ara02-pick",
        acts: &[laid(2)],
    },
    // A clip with its speed changed: the row is dim and says why.
    Shot {
        name: "ara03-cannot",
        acts: &[laid(3)],
    },
    // Attached and analysing: the turning ring and the plugin's percent.
    Shot {
        name: "ara04-busy",
        acts: &[laid(4)],
    },
    // Done: the plugin's mark.
    Shot {
        name: "ara05-ready",
        acts: &[laid(5)],
    },
    // 処理 › of an attached clip: edit and detach by the plugin's name.
    Shot {
        name: "ara06-attached",
        acts: &[laid(6)],
    },
    // The plugin went down: the same mark in the warning colour, and nothing else.
    Shot {
        name: "ara07-crashed",
        acts: &[laid(7)],
    },
    // A press on that badge: the plugin's small menu (its name, 編集 dim, 再起動, 外す).
    Shot {
        name: "ara08-restart",
        acts: &[laid(8)],
    },
    // It cannot be opened: the mark struck through.
    Shot {
        name: "ara09-failed",
        acts: &[laid(9)],
    },
    // Analysing in a clip too narrow for the percent: the ring alone, inside the clip, and the
    // name cut first.
    Shot {
        name: "ara10-narrow",
        acts: &[laid(10)],
    },
];

fn catalog() -> Catalog {
    Catalog {
        entries: vec![Entry {
            uid: UID.into(),
            name: NAME.into(),
            vendor: "Xirius".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            category: "Fx".into(),
            version: "1.0".into(),
            path: "sample.vst3".into(),
            ara: true,
            requires_process_main: false,
        }],
        failures: Vec::new(),
    }
}

fn vocal(s: &Studio) -> (xirius_core::id::TrackId, ClipId) {
    let t = track(s, "Vocal");
    (t, s.project().tracks[&t].clips[0].id)
}

fn report(clip: ClipId, phase: Phase, progress: f32) -> Report {
    Report {
        plugin: UID.into(),
        name: NAME.into(),
        loaded: true,
        crashed: None,
        failed: None,
        clips: 1,
        working: usize::from(phase != Phase::Ready),
        progress,
        phases: vec![(clip.0, phase)],
        editing: false,
    }
}

/// Right-clicks the Vocal clip and opens 処理 ›.
fn processing(s: &mut Studio, g: &Geometry) {
    s.popup = None;
    s.ctx = None;
    let (x, y) = clip_mid(s, g, "Vocal", 0);
    assert!(s.scene_context(g, x, y));
    let attached = s
        .selected_clip
        .is_some_and(|(_, c)| s.project().ara.clips.contains_key(&c));
    assert!(s.context_choose(
        g,
        if attached {
            "クリップ"
        } else {
            "オーディオ"
        }
    ));
}

fn labels(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| !r.gap)
        .map(|r| r.label)
        .collect()
}

/// Runs a command as a step of the ARA scripts, and fails the script when the window refuses
/// it or it fails: `run_command` is true when the window took the command, and a failure is
/// left in `error` (PLG-036).
pub(crate) fn run_ok(s: &mut Studio, command: &str, args: serde_json::Value) {
    s.error = None;
    let taken = s.run_command(command, args);
    assert!(taken, "{command} was refused: {:?}", s.error);
    assert_eq!(s.error, None, "{command} failed");
}

fn closed(s: &mut Studio) {
    s.popup = None;
    s.ctx = None;
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    if n == 0 {
        s.drawer_open = false;
        s.mixer.scan.auto = false;
        s.use_plugins(catalog());
        let g = &s.geometry(g.size.0, g.size.1);
        s.resized(g);
        return;
    }
    let g = &s.geometry(g.size.0, g.size.1);
    let (t, c) = vocal(s);
    match n {
        1 => {
            processing(s, g);
            assert_eq!(
                labels(s).last().map(String::as_str),
                Some(crate::clip_ara::ADD)
            );
        }
        2 => {
            assert!(s.context_choose(g, crate::clip_ara::ADD));
            assert!(
                s.popup
                    .as_ref()
                    .is_some_and(|p| p.kind == crate::popup::PopupKind::AraPlugin),
                "{:?}",
                s.error
            );
        }
        3 => {
            closed(s);
            run_ok(
                s,
                "clip.speed.set",
                json!({ "ids": [c.to_string()], "percent": 150 }),
            );
            processing(s, g);
            let row = s
                .context_rows()
                .into_iter()
                .find(|r| r.label == crate::clip_ara::ADD)
                .unwrap();
            assert!(!row.enabled);
        }
        4 => {
            closed(s);
            run_ok(
                s,
                "clip.speed.set",
                json!({ "ids": [c.to_string()], "percent": 100 }),
            );
            run_ok(
                s,
                "clip.ara.attach.apply",
                json!({ "ids": [c.to_string()], "plugin": UID, "name": NAME }),
            );
            s.ara.fake = Some(vec![report(c, Phase::Analysing, 0.42)]);
            assert!(s.ara_badge(c).is_some());
        }
        5 => {
            s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
        }
        6 => {
            processing(s, g);
            assert!(labels(s).iter().any(|l| l == &format!("{NAME}で編集")));
        }
        7 => {
            closed(s);
            let mut down = report(c, Phase::Ready, 1.0);
            down.crashed = Some("stopped".into());
            s.ara.fake = Some(vec![down]);
        }
        8 => {
            let (x, y) = s.center_of(g, Hit::Ara(t, c)).expect("the badge");
            let hit = s.hit(g, x, y);
            s.press(g, hit);
            assert_eq!(labels(s), ["オーディオ"]);
            assert!(s.context_choose(g, "オーディオ"));
            assert_eq!(labels(s), ["戻る", "編集", "再起動", "外す"]);
        }
        9 => {
            closed(s);
            let mut failed = report(c, Phase::Waiting, 0.0);
            failed.failed = Some("プラグインを読み込めません".into());
            s.ara.fake = Some(vec![failed]);
        }
        _ => {
            closed(s);
            s.ara.fake = Some(vec![report(c, Phase::Analysing, 0.42)]);
            let row = *s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
            let p = s.project();
            let clip = p.tracks[&t].clips[0].clone();
            let w = s.clip_rect(g, &row, &clip).w;
            s.beat_w *= 34.0 / w;
            let r = s.clip_rect(g, &row, &clip);
            let badge = s.ara_badge(c).unwrap();
            let at = s.ara_badge_rect(&p.tracks[&t], &clip, r, &badge);
            assert_eq!(at.w, crate::clip_ara::BOX_W);
            assert!(at.x >= r.x && at.right() <= r.right(), "{at:?} {r:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_plays_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 900.0);
        for n in 0..=10 {
            play(&mut s, &g, n);
        }
        // The stand-in is the only plugin the list shows.
        assert_eq!(s.ara_name(vocal(&s).1).as_deref(), Some(NAME));
    }
}
