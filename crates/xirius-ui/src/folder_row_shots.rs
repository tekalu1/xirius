//! UI-356: combined folder channels and the four legacy loading cases, without OS input.
use crate::{
    shot::{Act, Play, Shot},
    song::Song,
    studio::{DrawerTab, Geometry, Hit, Studio},
    tree::RowKind,
};
use serde_json::{Value, json};
use xirius_core::{
    id::{FolderId, Node, TrackId},
    model::Project,
    session::{RunOptions, Session},
    store::Store,
};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-open",
        acts: &[stage(0)],
    },
    Shot {
        name: "02-collapsed",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-output-hover",
        acts: &[stage(2)],
    },
    Shot {
        name: "04-output-menu",
        acts: &[stage(3)],
    },
    Shot {
        name: "05-inserts",
        acts: &[stage(4)],
    },
    Shot {
        name: "05a-track-output",
        acts: &[stage(5)],
    },
    Shot {
        name: "05b-track-output-hover",
        acts: &[stage(6)],
    },
    Shot {
        name: "05c-track-output-menu",
        acts: &[stage(7)],
    },
    Shot {
        name: "06-legacy-automatic",
        acts: &[stage(10)],
    },
    Shot {
        name: "07-legacy-organization",
        acts: &[stage(11)],
    },
    Shot {
        name: "08-legacy-shared",
        acts: &[stage(12)],
    },
    Shot {
        name: "09-legacy-renamed",
        acts: &[stage(13)],
    },
];

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}

fn fixture() -> Project {
    let mut s = Session::new("Folder channels");
    let drums = run(&mut s, "folder.add", json!({"name":"Drums"}));
    let cymbals = run(
        &mut s,
        "folder.add",
        json!({"name":"Cymbals", "into":drums["id"]}),
    );
    for (name, folder) in [("Kick", &drums), ("Snare", &drums), ("Hi-hat", &cymbals)] {
        let t = run(
            &mut s,
            "track.add.instrument",
            json!({"name":name, "into":folder["id"]}),
        );
        run(
            &mut s,
            "clip.add.midi",
            json!({"track":t["id"], "at":0, "bars":4}),
        );
    }
    run(&mut s, "folder.add", json!({"name":"Percussion"}));
    run(
        &mut s,
        "track.volume.set",
        json!({"ids":[drums["bus"]], "db":-3.0}),
    );
    run(
        &mut s,
        "plugin.add",
        json!({"track":drums["bus"], "name":"EQ"}),
    );
    s.project().clone()
}

fn load_legacy(mut p: Project, case: u8) -> Project {
    let ids: Vec<_> = p.folders.keys().copied().collect();
    for f in ids {
        let b = p.row_bus(f).unwrap();
        p.detach(Node::Track(b));
        let (parent, i) = p.find_node(Node::Folder(f)).unwrap();
        p.children_mut(parent).insert(i + 1, Node::Track(b));
    }
    let drums = folder(&p, "Drums");
    let bus = p.folders[&drums].bus.unwrap();
    match case {
        11 => {
            p.folders.get_mut(&drums).unwrap().bus = None;
        }
        12 => {
            let other = folder(&p, "Percussion");
            p.folders.get_mut(&other).unwrap().bus = Some(bus);
        }
        13 => {
            p.track_mut(bus).unwrap().name = "Drum mix".into();
        }
        _ => {}
    }
    let dir = crate::shot::script_dir(|| {
        std::env::temp_dir().join(format!("xirius-folder-row-{}", std::process::id()))
    });
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("project.json");
    let bytes = serde_json::to_vec(&json!({"format_version":1, "project":p})).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let loaded = Store::new(&dir).read_project().unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(loaded.row_bus(drums).is_some(), case == 10);
    for t in p.tracks.keys() {
        assert_eq!(loaded.effective_output(*t), p.effective_output(*t));
    }
    std::fs::remove_file(file).unwrap();
    std::fs::remove_dir(dir).unwrap();
    loaded
}

fn folder(p: &Project, name: &str) -> FolderId {
    p.folders.values().find(|f| f.name == name).unwrap().id
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    if n == 0 || n >= 10 {
        let p = if n >= 10 {
            load_legacy(fixture(), n)
        } else {
            fixture()
        };
        s.song = Song::imported_after(&s.song, p);
        s.song_replaced(Ok(None));
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        s.browser.open = false;
        s.resized(g);
        let f = folder(&s.project(), "Drums");
        s.select_row(Some(RowKind::Folder(f)));
        return;
    }
    let f = folder(&s.project(), "Drums");
    let b: TrackId = s.project().row_bus(f).unwrap();
    match n {
        1 => {
            s.press(g, Hit::Chevron(f));
        }
        2 => {
            s.hover = Hit::Output(b);
        }
        3 => {
            s.press(g, Hit::Output(b));
        }
        4 => {
            s.popup = None;
            s.hover = Hit::None;
            s.press(g, Hit::FolderBus(f));
        }
        5..=7 => {
            let t = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Kick")
                .unwrap()
                .id;
            if n == 5 {
                s.press(g, Hit::Chevron(f));
                s.press(g, Hit::Widen(t));
                s.hover = Hit::None;
            } else if n == 6 {
                s.hover = Hit::Output(t);
            } else {
                s.press(g, Hit::Output(t));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_scenes_read_the_saved_shape_without_writing_it() {
        let _dir = crate::shot::TestFolder::lent("xirius-folder-row-scenes-");
        for n in 10..=13 {
            super::load_legacy(super::fixture(), n)
                .validate_deep()
                .unwrap();
        }
    }
}
