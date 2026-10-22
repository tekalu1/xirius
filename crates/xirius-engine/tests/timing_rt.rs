//! Timing a plugin costs the audio thread no allocation and no lock (ENG-008).
use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use xirius_engine::timing::{Timed, Timings};
use xirius_engine::{AudioIo, NoteEvent, ProcessSetup, Processor, RenderMode};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

struct Quiet;
impl Processor for Quiet {
    fn name(&self) -> &str {
        "quiet"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
    }
}

#[test]
fn a_timed_block_allocates_nothing_even_after_the_ring_wraps() {
    let timings = Timings::new();
    let mut p = Timed::new(Box::new(Quiet), timings.meter(1));
    p.prepare(&ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 512,
        mode: RenderMode::Realtime,
    });
    let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
    assert_no_alloc(|| {
        for _ in 0..2000 {
            p.process(
                &mut AudioIo {
                    left: &mut l,
                    right: &mut r,
                },
                &[],
            );
        }
    });
    assert_eq!(
        timings.reading(1).expect("listed").blocks,
        // 64 frames is 1.3 ms: a second of them is more than the ring holds.
        xirius_engine::timing::RING as u32
    );
}
