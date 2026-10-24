//! DAWproject files written by the reference library (EXP-006, PLG-040): the README's song
//! opened on a PC without its Surge XT, the list that names the plugin, and an audio clip
//! with its fades. The OS's file windows are not opened: a stage gives the answer the window
//! would, and the plugin list is a scan of one other plugin.
use std::path::{Path, PathBuf};

use crate::{
    shot::{Act, Play, Shot},
    studio::{FileRequest, Geometry, Studio},
    toast::Answer,
};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Studio(prepare), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "dawproject-ref01-opened",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "dawproject-ref02-not-here",
        acts: &[Act::Stage(Play::Laid(press), 0)],
    },
    Shot {
        name: "dawproject-ref03-audio",
        acts: &[Act::Key("escape"), stage(1)],
    },
];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/dawproject-reference-ui-{}",
        std::process::id()
    ))
}

fn reference(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../xirius-core/src/dawproject/testdata/reference")
        .join(name)
}

/// The project of the DAWproject README (Bitwig 5.0), with its Surge XT state.
fn readme_song(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("Readme.dawproject");
    let mut zip =
        xirius_core::dawproject::zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let xml = include_str!("../../xirius-core/src/dawproject/testdata/bitwig-5.0-readme.xml");
    zip.add("project.xml", xml.as_bytes(), true).unwrap();
    zip.add(
        "plugins/d19b1f6e-bbb6-42fe-a6c9-54b41d97a05d.clap-preset",
        b"surge state",
        true,
    )
    .unwrap();
    zip.finish().unwrap();
    path
}

fn open(s: &mut Studio, path: &Path) {
    s.toast = None;
    s.finish_file_request(FileRequest::OpenDawproject, path);
    s.scroll.0 = 0.0;
}

fn prepare(s: &mut Studio, n: u8) {
    match n {
        0 => {
            // A PC that has scanned its plugins and has no Surge XT.
            s.use_plugins(xirius_plugins::Catalog {
                entries: vec![xirius_plugins::Entry {
                    uid: "com.example.other".into(),
                    name: "Other Synth".into(),
                    vendor: "Other".into(),
                    kind: xirius_core::model::PluginKind::Instrument,
                    format: "clap".into(),
                    category: String::new(),
                    version: "1".into(),
                    path: PathBuf::from("Other.clap"),
                    ara: false,
                }],
                failures: Vec::new(),
            });
            open(s, &readme_song(&dir()));
            s.after_open(|s| {
                let t = s.toast.as_ref().expect("what could not be read");
                assert!(t.alert, "{}", t.text);
            });
        }
        1 => {
            s.popup = None;
            open(s, &reference("AudioWARPEDWithFades.dawproject"));
            s.after_open(|s| {
                assert_eq!(
                    s.project()
                        .tracks
                        .values()
                        .map(|t| t.clips.len())
                        .sum::<usize>(),
                    1
                );
                s.beat_w = 96.0;
            });
        }
        _ => unreachable!(),
    }
}

/// Presses the toast's button: the list of what could not be read.
fn press(s: &mut Studio, g: &Geometry, _n: u8) {
    let parts = crate::toast::layout(s, g).expect("the toast");
    let b = parts.button.expect("its button");
    assert!(s.toast_press(g, b.x + b.w / 2.0, b.y + b.h / 2.0));
    let list = s.popup.as_ref().and_then(|p| p.skipped.as_ref());
    assert!(
        list.is_some_and(|l| l
            .rows
            .iter()
            .any(|(r, _)| r == "Bass の Surge XT（この PC に無い）")),
        "{:?}",
        list.map(|l| &l.rows)
    );
    assert!(matches!(
        s.toast.as_ref().and_then(|t| t.button.as_ref()),
        Some((_, Answer::Skipped(_)))
    ));
}
