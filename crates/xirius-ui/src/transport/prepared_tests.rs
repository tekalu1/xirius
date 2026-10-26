use super::*;
use crate::{browser_io_tests::Release, song::Song, studio::Studio};
use std::sync::mpsc;
use xirius_session::output::{DeviceOutput, SharedStream};

struct HeldOutput {
    audio: Option<xirius_engine::AudioEngine>,
    entered: mpsc::Sender<std::thread::ThreadId>,
    release: Arc<(Mutex<bool>, std::sync::Condvar)>,
}
impl SharedStream for HeldOutput {
    fn name(&self) -> String {
        "held test output".into()
    }
    fn lost(&self) -> xirius_engine::device::Lost {
        Default::default()
    }
    fn stop(mut self: Box<Self>) -> Option<xirius_engine::AudioEngine> {
        self.entered.send(std::thread::current().id()).unwrap();
        let held = self.release.0.lock().unwrap();
        drop(self.release.1.wait_while(held, |done| !*done).unwrap());
        self.audio.take()
    }
}

#[test]
fn zero_tracks_and_retiring_output_never_hold_the_ui_or_reopen_before_stop_completes() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.path().join("song")).unwrap();
    s.transport.mode = OutputMode::Device;
    let release = Release::new();
    let gate = release.0.clone();
    let (tx, entered) = mpsc::channel();
    let opens = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = opens.clone();
    let mut drivers = quiet_drivers();
    drivers.shared_out = Arc::new(move |audio, _, _| {
        count.fetch_add(1, Ordering::SeqCst);
        let tx = tx.clone();
        let gate = gate.clone();
        DeviceOutput::open_with(audio, move |audio| {
            Ok(Box::new(HeldOutput {
                audio: Some(audio),
                entered: tx,
                release: gate,
            }))
        })
        .map(Output::Device)
    });
    s.transport.set_drivers(drivers);
    s.prepare_song();
    assert_eq!(s.preparing.as_ref().unwrap().progress.units.len(), 0);
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "initial output watchdog");
        std::thread::yield_now();
    }
    assert!(s.transport.host().is_some());
    assert!(!s.transport.playing, "preparation never starts playback");
    s.transport.reset();
    let stopped_on = entered.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_ne!(stopped_on, std::thread::current().id());
    assert!(s.transport.host().is_none());
    s.prepare_song();
    for size in [(1280.0, 800.0), (900.0, 600.0)] {
        let g = s.geometry(size.0, size.1);
        assert!(s.poll_preparing());
        assert_eq!(s.preparing.as_ref().unwrap().layout(g.size).rect.w, 300.0);
        assert_eq!(opens.load(Ordering::SeqCst), 1);
    }
    s.prepare_key("escape");
    assert!(s.preparing.as_ref().unwrap().cancelling);
    assert!(s.poll_preparing());
    assert!(
        s.preparing.is_some(),
        "old output still belongs to its stop worker"
    );
    release.release();
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() || s.opening.is_some() {
        s.poll_preparing();
        s.poll_opening();
        assert!(Instant::now() < deadline, "stop cleanup watchdog");
        std::thread::yield_now();
    }
    assert!(s.song.dir().is_none());
    assert!(s.transport.host().is_none());
    assert_eq!(
        opens.load(Ordering::SeqCst),
        1,
        "cancelled output creation is never adopted or reopened"
    );
}

fn quiet_drivers() -> Drivers {
    Drivers {
        exclusive_out: None,
        shared_out: Arc::new(|audio, _, _| {
            Ok(Output::Silent(xirius_session::output::SilentOutput::start(
                audio, true,
            )))
        }),
        exclusive_in: None,
        shared_in: Arc::new(|_, _, _| Err("no test input".into())),
        devices: Arc::new(Vec::new),
    }
}

#[test]
fn native_material_gates_keep_preparation_input_and_cancel_responsive_until_cleanup() {
    for op in [
        "prepare_decode",
        "prepare_convert",
        "prepare_prime",
        "prepare_graph",
    ] {
        let root = tempfile::tempdir().unwrap();
        let wav = root.path().join("source.wav");
        let large = op == "prepare_prime";
        let audio = vec![0.1; if large { 4_200_000 } else { 2048 }];
        xirius_session::export::write_wav(
            &wav,
            &audio,
            &audio,
            if large { 48_000 } else { 22_050 },
            16,
        )
        .unwrap();
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("song")).unwrap();
        s.song
            .run("clip.import", serde_json::json!({"file":wav}))
            .unwrap();
        s.transport.mode = OutputMode::Device;
        s.transport.set_drivers(quiet_drivers());
        let release = Release::new();
        let gate = release.0.clone();
        let watch = root.path().to_path_buf();
        let seen = AtomicBool::new(false);
        let (tx, entered) = mpsc::channel();
        let ui = std::thread::current().id();
        let observe = xirius_core::io_test::observe(Arc::new(move |event, path| {
            if event == op && path.starts_with(&watch) && !seen.swap(true, Ordering::SeqCst) {
                tx.send(std::thread::current().id()).unwrap();
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |done| !*done).unwrap());
            }
            Ok(())
        }));
        s.prepare_song();
        assert!(s.preparing.as_ref().unwrap().background);
        let deadline = Instant::now() + Duration::from_secs(30);
        let worker = loop {
            s.poll_preparing();
            if let Ok(thread) = entered.try_recv() {
                break thread;
            }
            assert!(
                Instant::now() < deadline,
                "{op} entry watchdog: {:?}",
                s.error
            );
            std::thread::yield_now();
        };
        assert_ne!(worker, ui);
        assert!(s.transport.host().is_none());
        let original = s.project();
        for size in [(1200.0, 800.0), (880.0, 600.0)] {
            let g = s.geometry(size.0, size.1);
            assert!(s.poll_preparing());
            assert!(!s.run_command("track.add.audio", serde_json::json!({"name":"blocked"})));
            assert!(s.key(&g, "s", true, false));
            assert!(!s.toggle_play());
            assert!(!s.toggle_record());
            let layout = s.preparing.as_ref().unwrap().layout(g.size);
            assert!(layout.rect.right() <= size.0);
        }
        assert!(Arc::ptr_eq(&original, &s.project()));
        assert!(s.prepare_key("escape"));
        assert!(s.preparing.as_ref().unwrap().cancelling);
        assert!(s.poll_preparing());
        assert!(s.preparing.is_some(), "{op}: cleanup is still held");
        let card = s.preparing.as_ref().unwrap();
        let now = Instant::now();
        assert_eq!(card.bar(now), card.bar(now + Duration::from_secs(10)));
        release.release();
        let deadline = Instant::now() + Duration::from_secs(30);
        while s.preparing.is_some() || s.opening.is_some() {
            s.poll_preparing();
            s.poll_opening();
            assert!(Instant::now() < deadline, "{op} cleanup watchdog");
            std::thread::yield_now();
        }
        assert!(s.song.dir().is_none(), "{op}: cancellation closes the song");
        assert!(s.transport.host().is_none());
        assert!(!s.transport.playing);
        drop(observe);
    }
}
