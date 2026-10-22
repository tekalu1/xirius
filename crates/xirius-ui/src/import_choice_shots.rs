//! Import a tempo-tagged WAV through the palette's explicit tempo choice.
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::{Rect, Studio},
    topbar::BarButton,
};
use gpui::{
    App, Modifiers, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase, Window, point, px,
};
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use xirius_design::Layout;

pub const IMPORT_CHOICE: &[Shot] = &[
    Shot {
        name: "import-choice-00-command",
        acts: &[
            Act::Press(Target::Top(BarButton::Palette)),
            Act::Stage(Play::Window(play), 0),
        ],
    },
    Shot {
        name: "import-choice-01-file",
        acts: &[Act::Stage(Play::Window(play), 1)],
    },
    Shot {
        name: "import-choice-02-keep",
        acts: &[Act::Stage(Play::Window(play), 2)],
    },
    Shot {
        name: "import-choice-03-imported",
        acts: &[Act::Stage(Play::Window(play), 3)],
    },
    Shot {
        name: "import-choice-04-undone",
        acts: &[Act::Key("ctrl-z"), Act::Stage(Play::Window(play), 4)],
    },
];

fn fixture() -> PathBuf {
    std::env::temp_dir().join(format!("xirius-acid-choice-{}.wav", std::process::id()))
}
fn make_fixture() {
    let path = fixture();
    let samples = vec![0.0; 48000];
    xirius_session::export::write_wav(&path, &samples, &samples, 48000, 16).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let mut acid = Vec::new();
    acid.extend_from_slice(&0u32.to_le_bytes());
    acid.extend_from_slice(&60u16.to_le_bytes());
    acid.extend_from_slice(&0x8000u16.to_le_bytes());
    acid.extend_from_slice(&0f32.to_le_bytes());
    acid.extend_from_slice(&1u32.to_le_bytes());
    acid.extend_from_slice(&4u16.to_le_bytes());
    acid.extend_from_slice(&4u16.to_le_bytes());
    acid.extend_from_slice(&60f32.to_le_bytes());
    bytes.extend_from_slice(b"acid");
    bytes.extend_from_slice(&(acid.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&acid);
    let riff = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}
fn row(studio: &Studio, size: (f32, f32), index: usize) -> Rect {
    studio
        .palette
        .as_ref()
        .unwrap()
        .layout(size)
        .rows
        .iter()
        .find(|(i, _)| *i == index)
        .unwrap()
        .1
}
fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let size = window.viewport_size();
    let g = studio.borrow().geometry(
        f32::from(size.width),
        f32::from(size.height) - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H,
    );
    match n {
        0 => {
            make_fixture();
            studio
                .borrow_mut()
                .palette
                .as_mut()
                .unwrap()
                .replace_text(None, "clip.import");
        }
        1 => {
            let r = row(&studio.borrow(), g.size, 0);
            crate::aiarc_shots::click(r, window, cx);
            studio
                .borrow_mut()
                .palette
                .as_mut()
                .unwrap()
                .replace_text(None, &fixture().to_string_lossy());
        }
        2 => {
            let list = studio
                .borrow()
                .palette
                .as_ref()
                .unwrap()
                .layout(g.size)
                .list;
            window.dispatch_event(
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: point(px(list.x + 12.0), px(list.y + Layout::TOP_BAR_H + 12.0)),
                    delta: ScrollDelta::Pixels(point(px(0.0), px(-1000.0))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            );
            let index = studio
                .borrow()
                .palette
                .as_ref()
                .unwrap()
                .form
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .position(|field| field.name == "tempo")
                .unwrap();
            for _ in 0..3 {
                let r = row(&studio.borrow(), g.size, index);
                crate::aiarc_shots::click(r, window, cx);
            }
            assert_eq!(
                studio
                    .borrow()
                    .palette
                    .as_ref()
                    .unwrap()
                    .form
                    .as_ref()
                    .unwrap()
                    .fields[index]
                    .value,
                "keep"
            );
        }
        3 => {
            let index = studio
                .borrow()
                .palette
                .as_ref()
                .unwrap()
                .form
                .as_ref()
                .unwrap()
                .fields
                .len();
            let r = row(&studio.borrow(), g.size, index);
            crate::aiarc_shots::click(r, window, cx);
            assert!(studio.borrow().palette.is_none());
            assert_eq!(studio.borrow().project().tempo[0].bpm, 120.0);
            assert_eq!(imported_length(&studio.borrow()), Some(1920));
        }
        4 => {
            assert_eq!(imported_length(&studio.borrow()), None);
            xirius_session::peaks::wait();
            std::fs::remove_file(fixture()).unwrap();
        }
        _ => unreachable!(),
    }
    assert!(
        studio.borrow().error.is_none(),
        "{:?}",
        studio.borrow().error
    );
    eprintln!("import-choice-check stage={n} ok=true");
}
fn imported_length(studio: &Studio) -> Option<i64> {
    let file = fixture().to_string_lossy().into_owned();
    studio
        .project()
        .tracks
        .values()
        .flat_map(|track| track.clips.iter())
        .find_map(|clip| {
            if let xirius_core::model::ClipBody::Audio { file: imported, .. } = &clip.body {
                (imported == &file).then_some(clip.length)
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn palette_import_keeps_physical_duration_and_exposes_the_tempo_choice() {
        make_fixture();
        let mut studio = Studio::sample();
        let g = studio.geometry(1440.0, 820.0);
        assert!(
            studio.open_palette_form("clip.import", json!({"file": fixture().to_string_lossy()}))
        );
        let form = studio.palette.as_mut().unwrap().form.as_mut().unwrap();
        let tempo = form
            .fields
            .iter_mut()
            .find(|field| field.name == "tempo")
            .unwrap();
        assert_eq!(
            tempo
                .choices
                .iter()
                .map(|(raw, _)| raw.as_str())
                .collect::<Vec<_>>(),
            ["auto", "file", "keep"]
        );
        tempo.value = "keep".into();
        let before = studio.song.steps();
        studio.palette_form_enter(&g);
        assert!(studio.error.is_none(), "{:?}", studio.error);
        assert_eq!(studio.project().tempo[0].bpm, 120.0);
        assert_eq!(imported_length(&studio), Some(1920));
        assert_eq!(
            xirius_session::decode::probe(&fixture()).unwrap().frames,
            48000
        );
        assert_eq!(studio.song.steps(), before + 1);
        assert!(studio.undo(&g));
        assert_eq!(imported_length(&studio), None);
        xirius_session::peaks::wait();
        std::fs::remove_file(fixture()).unwrap();
    }
}
