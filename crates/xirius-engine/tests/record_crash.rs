//! A real killed writer leaves its flushed take recoverable (ENG-018), without a device.
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use xirius_engine::wavfile::{SafeWav, repair};

#[test]
fn writer_child() {
    let Some(path) = std::env::var_os("XIRIUS_CRASH_TAKE") else {
        return;
    };
    let path = PathBuf::from(path);
    let mut wav = SafeWav::create(&path, 2, 1000)
        .unwrap()
        .turning_rf64_after(4000);
    for i in 0..5000 {
        wav.write_sample(i as f32 / 5000.0).unwrap();
        wav.write_sample(-0.5).unwrap();
    }
    wav.sync().unwrap();
    std::fs::write(path.with_extension("ready"), b"ready").unwrap();
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

#[test]
fn a_forcibly_ended_recording_can_be_repaired_and_played() {
    let dir = tempfile::Builder::new()
        .prefix("xirius-killed-writer-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("take.wav");
    let ready = path.with_extension("ready");
    let mut child = Writer(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "writer_child", "--nocapture"])
            .env("XIRIUS_CRASH_TAKE", &path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let child = &mut child.0;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() && Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            panic!("the writer exited before flushing its take");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // This PID belongs to the test child created above, never to another user's process.
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(
        ready.exists(),
        "the writer did not finish its first disk flush"
    );
    let fixed = repair(&path).unwrap().unwrap();
    assert_eq!(fixed.frames, 5000);
    let audio = xirius_engine::wavfile::read_rf64(&path).unwrap().unwrap();
    assert_eq!(audio.channels[0].len(), 5000);
    assert_eq!(audio.channels[0][4321], 4321.0 / 5000.0);
    assert!(audio.channels[1].iter().all(|s| *s == -0.5));
}

/// The writer child, ended when dropped so a failed test leaves no process holding its take
/// (and the take's folder can go).
struct Writer(std::process::Child);

impl Drop for Writer {
    fn drop(&mut self) {
        // This PID belongs to the test child this test created, never to another process.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
