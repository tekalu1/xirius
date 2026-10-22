//! A failed process leaves records that a new UI process reaches through About.
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn crash_fixture() {
    if std::env::var_os("XIRIUS_UI_CRASH_FIXTURE").is_some() {
        xirius_ui::crash_reports::install().unwrap();
        panic!("UI crash fixture");
    }
}

fn guarded(mut command: Command) -> std::process::Output {
    // Files keep the child from blocking on a full pipe while the parent checks its progress.
    let dir = command
        .get_envs()
        .find(|(k, _)| *k == "XIRIUS_CRASH_DIR")
        .and_then(|(_, v)| v)
        .unwrap();
    let dir = std::path::PathBuf::from(dir);
    let out = dir.join("child.out");
    let err = dir.join("child.err");
    command
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(std::fs::File::create(&err).unwrap());
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("test child {} stalled", child.id());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    std::process::Output {
        status,
        stdout: std::fs::read(out).unwrap(),
        stderr: std::fs::read(err).unwrap(),
    }
}

#[test]
fn restarting_the_app_keeps_the_log_and_dump_reachable_from_the_window() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/ui-crash-entry-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let records = dir.join("records");
    std::fs::create_dir_all(&records).unwrap();
    let mut fixture = Command::new(std::env::current_exe().unwrap());
    fixture
        .args(["--exact", "crash_fixture", "--nocapture"])
        .env("XIRIUS_UI_CRASH_FIXTURE", "1")
        .env("XIRIUS_CRASH_DIR", &records);
    assert!(!guarded(fixture).status.success());
    let reports = xirius_engine::crash::reports_at(&records).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(
        std::fs::read_to_string(&reports[0].log)
            .unwrap()
            .contains("UI crash fixture")
    );
    assert!(
        std::fs::read(&reports[0].dump)
            .unwrap()
            .starts_with(b"MDMP")
    );
    let shots = dir.join("shots");
    let mut app = Command::new(env!("CARGO_BIN_EXE_xirius-ui"));
    app.args(["--park", "--silent", "--script", "crash-entry", "--shot"])
        .arg(&shots)
        .env("XIRIUS_CRASH_DIR", &records);
    let output = guarded(app);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("crash-entry records=1"), "{stderr}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 saved, 0 failed"));
    let picture = std::fs::read(shots.join("crash02-records.bmp")).unwrap();
    assert!(picture.len() > 100_000);
    assert!(
        picture[54..]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] != p[1])
    );
    std::fs::remove_dir_all(dir).unwrap();
}
