//! Pure timeline audio is cached ahead, invalidated by edits and disabled for live tracks.
use assert_no_alloc::{AllocDisabler, reset_violation_count, violation_count};
use std::sync::Arc;
use xirius_engine::{
    AudioData, AudioIo, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, Region, RenderMode,
    Timeline, TrackSlot, TrackSpec,
};
#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;
struct Plain;
impl Processor for Plain {
    fn name(&self) -> &str {
        "timeline"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
    }
    fn timeline_only(&self) -> bool {
        true
    }
}
fn timeline(value: f32) -> Timeline {
    Timeline::new(
        Vec::new(),
        vec![Region {
            start: 0,
            len: 4096,
            offset: 0,
            gain: 1.0,
            data: Arc::new(AudioData {
                left: vec![value; 4096],
                right: vec![value; 4096],
            }),
            shape: Default::default(),
        }],
    )
}

#[test]
fn variable_blocks_seeks_and_loop_positions_match_direct_timeline_samples() {
    let data = Arc::new(AudioData {
        left: (0..4096).map(|i| (i % 251) as f32 / 512.0).collect(),
        right: (0..4096).map(|i| -(i % 127) as f32 / 256.0).collect(),
    });
    let make = || {
        Timeline::new(
            Vec::new(),
            vec![Region {
                start: 13,
                len: 3000,
                offset: 7,
                gain: 0.75,
                data: data.clone(),
                shape: Default::default(),
            }],
        )
    };
    let reference = make();
    let mut track = TrackSpec::new("audio", Box::new(Plain));
    track.timeline = make();
    let mut plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(track)],
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 257,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    for (at, size) in [
        (0, 64),
        (64, 64),
        (128, 137),
        (265, 257),
        (997, 137),
        (13, 257),
        (270, 257),
        (13, 257),
        (2970, 137),
    ] {
        let (mut a, mut b, mut c, mut d) = ([0.0; 257], [0.0; 257], [0.0; 257], [0.0; 257]);
        reference.mix_audio(at, &mut a[..size], &mut b[..size]);
        plan.process_at(&mut c[..size], &mut d[..size], Some(at));
        assert_eq!(a, c, "left at {at}, size {size}");
        assert_eq!(b, d, "right at {at}, size {size}");
    }
}

#[test]
fn the_next_blocks_are_cached_and_an_edit_discards_them_without_allocating() {
    let mut t = TrackSpec::new("audio", Box::new(Plain));
    t.timeline = timeline(1.0);
    let mut p = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(t)],
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 64,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let (mut l, mut r) = ([0.0; 64], [0.0; 64]);
    reset_violation_count();
    assert_no_alloc::assert_no_alloc(|| p.process_at(&mut l, &mut r, Some(0)));
    assert_eq!(l, [1.0; 64]);
    let before = p.lookahead(0).unwrap();
    assert_eq!(before.cached_blocks, 2);
    assert_eq!(before.rendered_frames, 192);
    assert_no_alloc::assert_no_alloc(|| p.process_at(&mut l, &mut r, Some(64)));
    assert_eq!(
        p.lookahead(0).unwrap().rendered_frames,
        256,
        "one cached block consumed and one future block filled"
    );
    let edited = Box::new(timeline(2.0));
    reset_violation_count();
    let old = assert_no_alloc::assert_no_alloc(|| p.set_timeline(0, edited).unwrap());
    assert_eq!(p.lookahead(0).unwrap().cached_blocks, 0);
    assert_no_alloc::assert_no_alloc(|| p.process_at(&mut l, &mut r, Some(128)));
    assert_eq!(l, [2.0; 64]);
    assert!(p.lookahead(0).unwrap().generation > before.generation);
    assert_no_alloc::assert_no_alloc(|| {
        p.set_track_live(0, true);
        p.process_at(&mut l, &mut r, Some(192));
    });
    let live = p.lookahead(0).unwrap();
    assert_eq!(live.cached_blocks, 0);
    assert_no_alloc::assert_no_alloc(|| p.process_at(&mut l, &mut r, Some(256)));
    assert_eq!(
        p.lookahead(0).unwrap().rendered_frames,
        live.rendered_frames
    );
    assert_eq!(p.latency(), 0);
    assert_eq!(l, [2.0; 64]);
    assert_eq!(violation_count(), 0);
    drop(old);
}
