//! Muted tracks listed on completed mixdown and stem cards, through the export buttons.
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use serde_json::json;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "muted01-mixdown",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(stage), 0),
            Act::Press(Target::Mute("Drums")),
            Act::Press(Target::Top(BarButton::Export)),
            Act::Press(Target::PopupText("ループ範囲")),
            Act::Press(Target::PopupText("書き出す")),
            Act::Stage(Play::Studio(stage), 1),
        ],
    },
    Shot {
        name: "muted02-stems",
        acts: &[
            Act::Press(Target::PopupText("ステム")),
            Act::Press(Target::PopupText("書き出す")),
            Act::Stage(Play::Studio(stage), 1),
        ],
    },
];

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/export-muted-{}", std::process::id()))
}

fn stage(s: &mut Studio, n: u8) {
    match n {
        0 => {
            s.command(|song| song.save_as(&dir()));
            s.run_command("loop.set", json!({"start":0, "end":1920}));
        }
        1 => {
            crate::shot::run_for(s, 60.0, |s| !s.export.running());
            assert!(s.error.is_none(), "{:?}", s.error);
            let view = s.export_view();
            assert_eq!(
                view.cards[0].state,
                crate::export::State::Done,
                "{}",
                view.cards[0].error
            );
            assert!(view.cards[0].muted.iter().any(|n| n == "Drums"));
        }
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
    fn completed_exports_keep_the_muted_names_from_the_rendered_song() {
        let mut s = Studio::sample();
        stage(&mut s, 0);
        let drums = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Drums")
            .unwrap()
            .id;
        let g = s.geometry(1600.0, 900.0);
        s.press(&g, Hit::Mute(drums));
        s.press(&g, Hit::Bar(BarButton::Export));
        press(&mut s, Xp::Range(crate::export::Range::Loop));
        for stems in [false, true] {
            press(&mut s, Xp::Mode(stems));
            press(&mut s, Xp::Go);
            stage(&mut s, 1);
            assert_eq!(s.export_view().cards[0].muted, ["Drums"]);
        }
        s.command(|song| song.toggle_mute(drums));
        assert!(!s.project().tracks[&drums].mute);
        assert!(s.export_view().cards.iter().all(|c| c.muted == ["Drums"]));
        drop(s);
        std::fs::remove_dir_all(dir()).unwrap();
    }
}
