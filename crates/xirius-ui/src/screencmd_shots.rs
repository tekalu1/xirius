//! OPS-006 / CTX-001 / CTX-002: the screen's acts said as commands (`--ids`, `--at`, `--range`),
//! and a row of a menu that comes to the same command. Each shot ends on what the command did.
use crate::{
    arrange::probe::{folder, row_mid, track},
    popup::{Look, PopupKind},
    shot::{Act, Play, Shot},
    studio::{Geometry, Hit, Studio},
    tree::RowKind,
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "cmd01-folders-closed",
        acts: &[Act::Reset, Act::Stage(Play::Laid(stage), 0)],
    },
    Shot {
        name: "cmd02-lane-by-track-and-target",
        acts: &[Act::Stage(Play::Laid(stage), 1)],
    },
    Shot {
        name: "cmd03-export-of-a-range",
        acts: &[Act::Stage(Play::Laid(stage), 2)],
    },
    Shot {
        name: "cmd04-the-menu-row-is-the-command",
        acts: &[Act::Reset, Act::Stage(Play::Laid(stage), 3)],
    },
];

/// A press on the row of a menu that is open, as the window presses it.
fn choose(s: &mut Studio, g: &Geometry, label: &str) {
    let layout = s.popup.as_ref().expect("a menu is open").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.text == label && !matches!(c.look, Look::Input))
        .unwrap_or_else(|| panic!("no row {label}"));
    let hit = s.hit(
        g,
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    );
    assert!(matches!(hit, Hit::Item(_)), "{label}: {hit:?}");
    s.press(g, hit);
}

fn typed(s: &mut Studio, g: &Geometry, line: &str) {
    assert!(s.run_binding(g, line), "{line}: {:?}", s.error);
    assert!(s.error.is_none(), "{line}: {:?}", s.error);
}

fn stage(s: &mut Studio, g: &Geometry, n: u8) {
    s.arrange.drag = None;
    s.popup = None;
    s.ctx = None;
    s.drawer_open = false;
    match n {
        0 => {
            typed(s, g, "view.folders --state closed");
            let p = s.project();
            assert!(p.folders.keys().all(|f| s.is_collapsed(*f)));
        }
        1 => {
            typed(s, g, "view.folders --state open");
            let bass = track(s, "Bass");
            typed(s, g, &format!("auto.lane.show --track {bass} --target pan"));
            assert!(s.arrange.open.contains(&bass));
            assert!(
                s.project().tracks[&bass]
                    .automation
                    .iter()
                    .any(|l| l.target.to_string() == "pan" && !l.hidden)
            );
        }
        2 => {
            typed(s, g, "view.export --range 2.1.0..4.1.0");
            assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Export));
            assert!(s.export.fixed_span.is_some());
        }
        3 => {
            let f = folder(s, "Rhythm");
            s.command(|song| {
                song.run("folder.bus.add", serde_json::json!({ "id": f.to_string() }))
                    .map(|_| ())
            });
            let (x, y) = row_mid(s, g, RowKind::Folder(f));
            assert!(s.scene_context(g, x, y));
            choose(s, g, "ステムを書き出す");
            choose(s, g, "このフォルダーを 1 本に…");
            assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Export));
            assert!(s.export.stems && s.export.folders == [f]);
        }
        _ => unreachable!(),
    }
}
