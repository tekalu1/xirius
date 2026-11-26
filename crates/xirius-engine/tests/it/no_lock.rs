use std::sync::Arc;
use xirius_engine::{
    AudioIo, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode, TrackSlot, TrackSpec,
    engine::{self, EngineConfig},
    rt_lock::Mutex,
};

#[test]
fn preview_feeding_is_rejected_in_an_audio_callback() {
    let setup = ProcessSetup {
        sample_rate: 48000.0,
        max_block: 64,
        mode: RenderMode::Realtime,
    };
    let (mut audio, control) = engine::create(
        xirius_engine::audition::plan(setup).unwrap(),
        EngineConfig::default(),
    );
    let port = control.audition();
    port.start(1);
    port.feed(1, &[0.25; 64], &[0.5; 64]);
    let mut out = [0.0; 128];
    audio.render(&mut out, 2);
    assert_eq!(out[0..2], [0.25, 0.5]);
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _audio = xirius_engine::rt_lock::AudioScope::enter();
        port.feed(1, &[0.0], &[0.0]);
    }));
    assert!(
        rejected.is_err(),
        "the preview producer lock must be checked too"
    );
}

struct LockingProcessor(Arc<Mutex<()>>);
impl Processor for LockingProcessor {
    fn name(&self) -> &str {
        "locking processor"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, _: &mut AudioIo<'_>, _: &[NoteEvent]) {
        let _held = self.0.lock().unwrap();
    }
}

#[test]
fn acquiring_a_lock_inside_the_actual_render_callback_panics() {
    let lock = Arc::new(Mutex::new(()));
    {
        let _held = lock.lock().unwrap();
    }
    let setup = ProcessSetup {
        sample_rate: 48000.0,
        max_block: 64,
        mode: RenderMode::Realtime,
    };
    let plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(TrackSpec::new(
                "locked",
                Box::new(LockingProcessor(lock.clone())),
            ))],
            ..MixSpec::default()
        },
        setup,
        None,
    )
    .unwrap();
    let (mut audio, _) = engine::create(plan, EngineConfig::default());
    let mut output = [0.0; 128];
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        audio.render(&mut output, 2)
    }));
    assert!(result.is_err());
    assert!(
        lock.lock().is_ok(),
        "the audio scope unwinds and the lock was never poisoned"
    );
}

#[test]
fn both_blocking_and_try_lock_are_rejected_and_nested_scope_unwinds() {
    let lock = Mutex::new(());
    let result = std::panic::catch_unwind(|| {
        let _outer = xirius_engine::rt_lock::AudioScope::enter();
        {
            let _inner = xirius_engine::rt_lock::AudioScope::enter();
        }
        let _held = lock.try_lock();
    });
    assert!(result.is_err());
    assert!(lock.try_lock().is_ok());
}
