//! The ARA document and its samples live in the plugin-host process (PLG-078).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_ara::fake;
use xirius_ara::remote_client::RemoteService;
use xirius_ara::service::source_id;
use xirius_ara::shared_audio::SharedAudio;
use xirius_ara::{ClipSpec, Phase, Service, Song, SourceAudio};

mod crash_records;
use crash_records::CrashRecords;

const WAIT: Duration = Duration::from_secs(20);

fn host() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_xirius-plugin-host"))
}

fn open_remote(archive: Option<(Vec<u8>, String)>, fixture: fake::Config) -> RemoteService {
    RemoteService::start_with_fixture(
        host(),
        "fake",
        Path::new(""),
        "fixture",
        "fixture",
        archive,
        Arc::new(|| {}),
        Some(fixture),
    )
}

fn make_song(audio: Arc<SourceAudio>) -> Song {
    Song {
        clips: vec![ClipSpec {
            regions: Vec::new(),
            key: 42,
            clone_of: None,
            source: source_id("source.wav"),
            name: "source".into(),
            audio,
            start: 0.0,
            offset: 0.0,
            duration: 1.0,
        }],
        ..Song::default()
    }
}

fn audio() -> Arc<SourceAudio> {
    let left: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let right = left.iter().map(|v| -*v).collect();
    Arc::new(SourceAudio::stereo(48_000.0, left, right))
}

fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn analysis_edit_archive_and_render_cross_the_host_boundary() {
    let _crash_records = CrashRecords::new();
    let song = make_song(audio());
    let local = Service::start(fake::loader(Default::default()), None, Arc::new(|| {}));
    local.sync(&song);
    assert!(local.wait_idle(Instant::now() + WAIT));
    let reference = local.state(42).unwrap().rendered.unwrap();

    let remote = open_remote(None, Default::default());
    remote.sync(&song);
    assert!(
        remote.wait_idle(Instant::now() + WAIT),
        "{:?}",
        remote.crashed().or(remote.failed())
    );
    let render = remote.state(42).unwrap().rendered.unwrap();
    assert_eq!(render.left, reference.left, "the exported samples changed");
    assert_eq!(
        render.right, reference.right,
        "the exported samples changed"
    );
    assert!(!remote.notes(42, false, WAIT).is_empty());
    assert!(remote.probe(WAIT).unwrap().reads_off_thread > 0);
    let source = source_id("source.wav");
    let first_name = remote.source_mapping_name_for_test(&source).unwrap();
    assert!(SharedAudio::open(&first_name).is_ok());

    remote.fixture_edit(42, 0.5);
    until("edited render", || {
        remote
            .state(42)
            .and_then(|s| s.rendered)
            .is_some_and(|r| r.left != reference.left)
    });
    let edited = remote.state(42).unwrap().rendered.unwrap();
    let archive = remote.store(WAIT).unwrap();
    drop(remote);
    until("closed source mapping", || {
        SharedAudio::open(&first_name).is_err()
    });

    let restored = open_remote(Some(archive), Default::default());
    restored.sync(&song);
    assert!(restored.wait_idle(Instant::now() + WAIT));
    let again = restored.state(42).unwrap().rendered.unwrap();
    assert_eq!(again.left, edited.left);
    assert_eq!(again.right, edited.right);
    assert_eq!(restored.probe(WAIT).unwrap().analysis_starts, 0);

    let replacement = Arc::new(SourceAudio::stereo(
        48_000.0,
        vec![0.25; 48_000],
        vec![-0.25; 48_000],
    ));
    restored.sync(&make_song(replacement));
    assert!(restored.wait_idle(Instant::now() + WAIT));
    let new_name = restored.source_mapping_name_for_test(&source).unwrap();
    assert_ne!(first_name, new_name);
    assert!(SharedAudio::open(&new_name).is_ok());
    drop(restored);
    until("replaced source mapping", || {
        SharedAudio::open(&new_name).is_err()
    });
}

#[test]
fn ending_the_host_during_analysis_keeps_the_client_alive_and_allows_restart() {
    let _crash_records = CrashRecords::new();
    let song = make_song(audio());
    let slow = open_remote(
        None,
        fake::Config {
            analysis_ms: 3000,
            ..Default::default()
        },
    );
    slow.sync(&song);
    until("analysis", || {
        assert!(slow.crashed().is_none(), "{:?}", slow.crashed());
        assert!(slow.failed().is_none(), "{:?}", slow.failed());
        slow.state(42).is_some_and(|s| s.phase == Phase::Analysing)
    });
    slow.terminate_host_for_test();
    until("host exit", || slow.crashed().is_some());
    assert_eq!(slow.state(42).unwrap().phase, Phase::Crashed);
    drop(slow);
    let restarted = open_remote(None, Default::default());
    restarted.sync(&song);
    assert!(restarted.wait_idle(Instant::now() + WAIT));
    assert_eq!(restarted.state(42).unwrap().phase, Phase::Ready);
}

#[test]
fn ending_the_host_during_render_keeps_the_client_alive_and_allows_restart() {
    let _crash_records = CrashRecords::new();
    let mut song = make_song(audio());
    song.clips[0].duration = 3.0;
    let slow = open_remote(
        None,
        fake::Config {
            render_spin_us: 10_000,
            ..Default::default()
        },
    );
    slow.sync(&song);
    until("render", || {
        assert!(slow.crashed().is_none(), "{:?}", slow.crashed());
        assert!(slow.failed().is_none(), "{:?}", slow.failed());
        slow.state(42).is_some_and(|s| s.phase == Phase::Rendering)
    });
    slow.terminate_host_for_test();
    until("host exit", || slow.crashed().is_some());
    assert_eq!(slow.state(42).unwrap().phase, Phase::Crashed);
    drop(slow);
    let restarted = open_remote(None, Default::default());
    restarted.sync(&song);
    assert!(restarted.wait_idle(Instant::now() + WAIT));
    assert_eq!(restarted.state(42).unwrap().phase, Phase::Ready);
}
