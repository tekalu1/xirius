use std::sync::Arc;
use xirius_engine::{
    AudioIo, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode, TrackSlot, TrackSpec,
    engine::{self, EngineConfig},
    rt_lock::Mutex,
};

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
