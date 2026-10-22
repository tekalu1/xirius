//! Select a clip in the arrangement, then export its visible range.
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Studio(prepare), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "selection01-range",
        acts: &[
            Act::Reset,
            stage(0),
            Act::Press(Target::Clip("Keys")),
            Act::Press(Target::Top(BarButton::Export)),
            Act::Press(Target::PopupText("選択範囲")),
            stage(1),
        ],
    },
    Shot {
        name: "selection02-written",
        acts: &[Act::Press(Target::PopupText("書き出す")), stage(2)],
    },
    Shot {
        name: "selection03-cleared",
        acts: &[
            Act::Key("escape"),
            Act::Key("ctrl-d"),
            Act::Press(Target::Top(BarButton::Export)),
            stage(3),
        ],
    },
];

fn prepare(s: &mut Studio, n: u8) {
    match n {
        0 => {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../target/export-selection-ui-{}",
                std::process::id()
            ));
            s.command(|song| song.save_as(&dir));
            s.drawer_open = false;
        }
        1 => {
            let (start, end) =
                xirius_session::export_selection::span(&s.project(), &s.export_clip_ids()).unwrap();
            assert_eq!(s.export_view().span(), (start, end));
            assert_eq!(s.export.range, crate::export::Range::Selection);
            assert!(s.export_view().can_go());
        }
        2 => {
            crate::shot::run_for(s, 60.0, |s| !s.export.running());
            assert!(s.error.is_none(), "{:?}", s.error);
            let (start, end) = s.export_view().span();
            let job = &s.export.jobs[0];
            assert_eq!(job["state"], "done");
            assert_eq!(job["args"]["from"], start);
            assert_eq!(job["args"]["to"], end);
        }
        3 => {
            s.poll_export();
            assert!(s.export_clip_ids().is_empty());
            assert!(!s.popup.as_ref().unwrap().export.as_ref().unwrap().can_go());
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export::Range, studio::Hit, tree::RowKind};
    use serde_json::json;

    #[test]
    fn the_panel_and_palette_use_clip_bounds_and_reopening_keeps_the_snapshot() {
        let mut s = Studio::sample();
        prepare(&mut s, 0);
        let g = s.geometry(1600.0, 900.0);
        s.arrange.clips.clear();
        s.selected_clip = None;
        assert!(!s.run_binding(&g, "export.selection"));
        assert!(
            !s.palette_rows(&g)
                .iter()
                .find(|r| r.name == "export.selection")
                .unwrap()
                .enabled
        );
        let project = s.project();
        let track = project.tracks.values().find(|t| t.name == "Keys").unwrap();
        let (x, y) = s
            .center_of(&g, Hit::Clip(track.id, track.clips[0].id))
            .unwrap();
        s.arrange_down(&g, x, y, crate::tools::Mods::default());
        s.arrange_up(&g, x, y, crate::tools::Mods::default());
        assert!(
            s.palette_rows(&g)
                .iter()
                .find(|r| r.name == "export.selection")
                .unwrap()
                .enabled
        );
        let row = s
            .palette_rows(&g)
            .into_iter()
            .find(|r| r.name == "export.selection")
            .unwrap();
        assert!(s.run_row(&g, &row));
        prepare(&mut s, 1);
        let span = s.export_view().span();
        s.export_action(crate::export::Act::Go);
        prepare(&mut s, 2);
        let job = s.export.jobs[0]["job"].as_str().unwrap().to_string();
        assert!(s.run_binding(&g, "select.clear"));
        assert!(s.export_clip_ids().is_empty());
        s.poll_export();
        assert!(!s.popup.as_ref().unwrap().export.as_ref().unwrap().can_go());
        assert!(
            s.selected_clip.is_some(),
            "editing a clip does not select it"
        );
        s.selected_row = Some(RowKind::Track(track.id));
        assert!(s.export_like(&job));
        assert_eq!(s.export.range, Range::Selection);
        assert_eq!(s.export_view().span(), span);
        assert!(s.export_view().can_go());
        s.export_action(crate::export::Act::Range(Range::Selection));
        assert!(!s.export_view().can_go());
        assert!(s.song.run("export.selection", json!({"ids":[]})).is_err());
        let map = s.project().time_map();
        s.export.jobs[0]["args"]["from"] = json!(map.format(span.0));
        s.export.jobs[0]["args"]["to"] = json!(map.format(span.1));
        assert!(s.export_like(&job));
        assert_eq!(s.export_view().span(), span);
        assert!(s.export_view().can_go());
        s.export.jobs[0]["args"]["to"] = json!("invalid");
        assert!(!s.export_like(&job));
        let dir = s.song.dir().unwrap();
        s.new_song();
        s.command(|song| song.save_as(&dir.join("other")));
        s.popup = None;
        s.press(&g, Hit::Bar(BarButton::Export));
        assert_eq!(s.export.range, Range::Song);
        assert!(s.export.fixed_span.is_none());
        drop(s);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
