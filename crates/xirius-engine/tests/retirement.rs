//! A real retired plan may block in destruction; callers can hand it to a worker.
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use xirius_engine::{
    AudioIo, Command, EngineConfig, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode,
    TrackSlot, TrackSpec, engine::create,
};

struct HeldDrop {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl Processor for HeldDrop {
    fn name(&self) -> &str {
        "held retirement"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
    }
}
impl Drop for HeldDrop {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let _ = self.release.recv();
    }
}
#[test]
fn a_retired_plan_can_block_on_a_worker_while_the_controller_and_audio_continue() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Offline,
    };
    let plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(TrackSpec::new(
                "A",
                Box::new(HeldDrop {
                    entered: entered_tx,
                    release: release_rx,
                }),
            ))],
            ..Default::default()
        },
        setup,
        None,
    )
    .unwrap();
    let (mut audio, mut controller) = create(plan, EngineConfig::default());
    controller.swap(MixSpec::default()).unwrap();
    audio.render(&mut [0.0; 512], 2);
    let retired = controller.poll_retiring(|_| {});
    assert!(!retired.is_empty());
    let worker = std::thread::spawn(move || drop(retired));
    entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    for _ in 0..32 {
        assert!(controller.send(Command::Play).is_ok());
        audio.render(&mut [0.0; 512], 2);
        assert!(controller.poll_retiring(|_| {}).is_empty());
    }
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !worker.is_finished() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    worker.join().unwrap();
}
