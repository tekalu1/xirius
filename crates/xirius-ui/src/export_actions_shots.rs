//! Listen and import on the written file's row, through real window presses.
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use serde_json::json;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Studio(prepare), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "actions01-written",
        acts: &[
            Act::Reset,
            stage(0),
            Act::Press(Target::Top(BarButton::Export)),
            Act::Press(Target::PopupText("ループ範囲")),
            Act::Press(Target::PopupText("書き出す")),
            stage(1),
        ],
    },
    Shot {
        name: "actions02-listening",
        acts: &[Act::Press(Target::PopupText("聴く")), stage(2)],
    },
    Shot {
        name: "actions03-imported",
        acts: &[
            Act::Press(Target::PopupText("停止")),
            Act::Press(Target::PopupText("曲に取り込む")),
            Act::Key("escape"),
            stage(3),
        ],
    },
    Shot {
        name: "actions04-stale",
        acts: &[Act::Press(Target::Top(BarButton::Export)), stage(4)],
    },
];

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/export-actions-ui-{}",
        crate::shot::fixture_run_id()
    ))
}

fn prepare(s: &mut Studio, n: u8) {
    match n {
        0 => {
            s.command(|song| song.save_as(&dir()));
            s.run_command("loop.set", json!({"start":15360,"end":30720}));
        }
        1 => {
            crate::shot::run_for(s, 60.0, |s| !s.export.running());
            assert_eq!(s.export_view().cards[0].state, crate::export::State::Done);
        }
        2 => {
            crate::shot::run_for(s, 60.0, |s| s.export.preview_peak > 0.01);
            assert!(s.export.preview_peak > 0.01, "{:?}", s.error);
            assert!(s.export.preview.is_some());
            assert!(!s.transport.is_live());
            assert!(s.transport.auditioning_export);
        }
        3 => {
            assert!(s.error.is_none(), "{:?}", s.error);
            assert_eq!(s.project().tracks.len(), 9);
            let last = s.tree().rows.last().unwrap();
            s.scroll.1 = last.y;
            s.drawer_open = false;
        }
        4 => assert_eq!(s.export_view().cards[0].stale, Some(true)),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export::Act as Xp, popup::Action, studio::Hit};

    fn press(s: &mut Studio, act: Xp) {
        let g = s.geometry(1600.0, 900.0);
        let cells = s.popup.as_ref().unwrap().layout(g.size).cells;
        let i = cells
            .iter()
            .position(|c| c.live() && c.action == Some(Action::Xp(act.clone())))
            .unwrap();
        let r = cells[i].rect;
        let hit = s.hit(&g, r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert_eq!(hit, Hit::Item(i));
        s.press(&g, hit);
    }

    #[test]
    fn the_file_row_auditions_to_silence_and_imports_at_the_playhead_in_one_step() {
        let mut s = Studio::sample();
        s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
        prepare(&mut s, 0);
        let before = s.project();
        let g = s.geometry(1600.0, 900.0);
        s.press(&g, Hit::Bar(BarButton::Export));
        press(&mut s, Xp::Range(crate::export::Range::Loop));
        press(&mut s, Xp::Go);
        prepare(&mut s, 1);
        let job = s.export_view().cards[0].job.clone();
        s.toggle_play();
        assert!(s.transport.is_live());
        press(&mut s, Xp::Preview(job.clone()));
        prepare(&mut s, 2);
        press(&mut s, Xp::Preview(job.clone()));
        assert!(s.export.preview.is_none());
        assert!(!s.transport.auditioning_export);
        press(&mut s, Xp::Import(job));
        assert_eq!(s.project().tracks.len(), before.tracks.len() + 1);
        let project = s.project();
        let new = project
            .tracks
            .values()
            .find(|t| !before.tracks.contains_key(&t.id))
            .unwrap();
        assert_eq!(new.clips[0].start, s.playhead);
        assert!(s.song.undo());
        assert_eq!(s.project(), before);
        drop(s);
        std::fs::remove_dir_all(dir()).unwrap();
    }
}
