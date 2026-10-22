//! A held disk writer produces a real gap, displayed in the recording area.

use crate::{
    shot::{Act, Play, Shot},
    studio::{Hit, Studio},
    tree::RowKind,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, Window, point, px,
};
use serde_json::json;
use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};
use xirius_design::Layout;
use xirius_engine::record::{AudioLane, MockInput, RecordSpec, Recorder};
use xirius_engine::{Command, MixSpec, Plan, ProcessSetup, RenderMode};

pub const SHOTS: &[Shot] = &[Shot {
    name: "loss01-recording-gap",
    acts: &[Act::Reset, Act::Stage(Play::Window(play), 0)],
}];

pub fn delayed_recording(dir: &Path) -> xirius_engine::record::Recording {
    std::fs::create_dir_all(dir).unwrap();
    let setup = ProcessSetup {
        sample_rate: 48000.0,
        max_block: 256,
        mode: RenderMode::Realtime,
    };
    let plan = Plan::build(MixSpec::default(), setup, None).unwrap();
    let (mut audio, mut control) = xirius_engine::engine::create(plan, Default::default());
    let (release, held) = std::sync::mpsc::channel();
    let (entered, started) = std::sync::mpsc::channel();
    let dir = dir.to_path_buf();
    let mut gate = Some((entered, held));
    let (recorder, capture) = Recorder::start(RecordSpec {
        sample_rate: 48000,
        lanes: vec![AudioLane {
            channels: vec![0],
            path: Box::new(move |k| {
                if let Some((entered, held)) = gate.take() {
                    entered.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(60)).unwrap();
                }
                dir.join(format!("gap-{k}.wav"))
            }),
        }],
        midi: false,
        latency: 0,
        midi_latency: 0,
        align: Box::new(|f| f),
        ring_seconds: 0.02,
        thru: vec![],
    })
    .unwrap();
    control
        .send(Command::SetInput(Some(Box::new(MockInput::sine(
            48000.0, 440.0, 0.25,
        )))))
        .ok()
        .expect("input command");
    control
        .send(Command::Record(Some(capture)))
        .ok()
        .expect("capture command");
    control.send(Command::Play).ok().expect("play command");
    let mut out = [0.0; 512];
    audio.render(&mut out, 2);
    started.recv_timeout(Duration::from_secs(60)).unwrap();
    for _ in 0..64 {
        audio.render(&mut out, 2);
        control.poll(|_| {});
    }
    release.send(()).unwrap();
    control
        .send(Command::Record(None))
        .ok()
        .expect("stop command");
    audio.render(&mut out, 2);
    let done = recorder.finish(Duration::from_secs(60));
    assert!(done.detached && done.errors.is_empty(), "{:?}", done.errors);
    assert!(done.dropped > 0);
    done
}

fn play(_: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/record-loss-shots");
    let done = delayed_recording(&dir);
    let (position, t) = {
        let mut s = studio.borrow_mut();
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Vocal")
            .unwrap()
            .id;
        let takes = done.takes.iter().flat_map(|take| take.audio.iter()).map(|a| json!({
            "track": t.to_string(), "start": 0, "length": 960, "file": a.path.display().to_string()
        })).collect::<Vec<_>>();
        s.command(|song| {
            song.run("record.take.add", json!({"takes": takes}))
                .map(|_| ())
        });
        s.drawer_open = false;
        s.scroll.1 = s.tree().row_of(RowKind::Track(t)).unwrap().y;
        s.scroll.0 = 0.0;
        let g = crate::screen::geometry(&s, window);
        (s.center_of(&g, Hit::Arm(t)).unwrap(), t)
    };
    let position = point(px(position.0), px(position.1 + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
    let mut s = studio.borrow_mut();
    assert!(s.project().tracks[&t].armed);
    let finished = xirius_session::record::Finished {
        dropped: done.dropped,
        dropped_blocks: done.dropped_blocks,
        dropouts: done.dropouts,
        ..Default::default()
    };
    s.transport.loss.tracks = vec![t];
    let p = s.project();
    s.transport.loss.receive(&finished.to_json(), &p, 48000);
    assert!(s.transport.loss.label(&p).contains("回"));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_held_writer_reports_the_exact_missing_positions_and_callback_count() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/record-loss-test-{}",
            std::process::id()
        ));
        let done = delayed_recording(&dir);
        assert_eq!(
            done.dropouts.iter().map(|r| r.frames).sum::<u64>(),
            done.dropped
        );
        assert_eq!(
            done.dropouts.iter().map(|r| r.blocks).sum::<u64>(),
            done.dropped_blocks
        );
        let finished = xirius_session::record::Finished {
            dropped: done.dropped,
            dropped_blocks: done.dropped_blocks,
            dropouts: done.dropouts,
            ..Default::default()
        };
        let p = xirius_core::model::Project::new("test");
        let mut loss = crate::record_loss::Loss::default();
        loss.receive(&finished.to_json(), &p, 48000);
        assert_eq!(loss.blocks, finished.dropped_blocks);
        assert!(!loss.positions.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
