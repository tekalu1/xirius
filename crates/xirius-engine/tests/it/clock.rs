//! Audio regions, scheduled MIDI and the click share the song's frame clock (ENG-035).
use std::sync::Arc;
use xirius_engine::click::Click;
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::{
    AudioData, AudioIo, Command, MixSpec, NoteEvent, NoteKind, Plan, ProcessSetup, Processor,
    Region, RenderMode, TimedNote, Timeline, TrackSlot, TrackSpec,
};

struct Impulse;
impl Processor for Impulse {
    fn name(&self) -> &str {
        "MIDI impulse"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        io.clear();
        for e in events {
            if matches!(e.kind, NoteKind::On { .. }) {
                io.right[e.offset as usize] = 1.0;
            }
        }
    }
}

fn rendered(callback: usize, block: usize) -> Vec<f32> {
    let frames = 20_000;
    let beats = [0, 4800, 9600, 14_400];
    let mut data = AudioData {
        left: vec![0.0; frames],
        right: vec![0.0; frames],
    };
    for f in beats {
        data.left[f] = 1.0;
    }
    let mut t = TrackSpec::new("clock", Box::new(Impulse));
    t.timeline = Timeline::new(
        beats
            .into_iter()
            .map(|frame| TimedNote {
                frame: frame as u64,
                kind: NoteKind::On {
                    pitch: 60,
                    velocity: 1.0,
                },
            })
            .collect(),
        vec![Region {
            start: 0,
            len: frames as u64,
            offset: 0,
            gain: 1.0,
            data: Arc::new(data),
            shape: Default::default(),
        }],
    );
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: block,
        mode: RenderMode::Offline,
    };
    let (mut audio, mut c) = engine::create(
        Plan::build(
            MixSpec {
                tracks: vec![TrackSlot::New(t)],
                ..Default::default()
            },
            setup,
            None,
        )
        .unwrap(),
        EngineConfig::default(),
    );
    c.send(Command::SetClick(Some(Box::new(Click::new(
        48_000.0,
        beats
            .into_iter()
            .enumerate()
            .map(|(i, f)| (f as u64, i as u32))
            .collect(),
        4800.0,
        4,
        0.2,
    )))))
    .ok()
    .unwrap();
    c.send(Command::Seek(123)).ok().unwrap();
    c.send(Command::SetLoop(Some((123, 15_001)))).ok().unwrap();
    c.send(Command::Play).ok().unwrap();
    let mut out = vec![0.0; 50_000 * 2];
    for chunk in out.chunks_mut(callback * 2) {
        audio.render(chunk, 2);
        c.poll(|_| {});
    }
    // Audio and MIDI impulses meet at every scheduled beat, including after loop wraps.
    for frame in out.as_chunks::<2>().0 {
        assert_eq!(frame[0], frame[1]);
    }
    out
}

#[test]
fn audio_midi_and_click_do_not_drift_across_buffers_or_loop_wraps() {
    let reference = rendered(256, 256);
    for (callback, block) in [(137, 64), (997, 512), (1024, 257)] {
        assert_eq!(
            rendered(callback, block),
            reference,
            "callback {callback}, block {block}"
        );
    }
    assert!(
        reference.iter().any(|x| *x >= 1.0),
        "the scheduled impulses actually played"
    );
}
