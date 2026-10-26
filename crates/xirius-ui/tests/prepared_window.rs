//! Parked GPUI/PrintWindow probe; real material I/O is held while Esc is dispatched
//! to this window. No global input, physical audio, or personal song is used.
#![cfg(feature = "loading-probe")]

use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn material_gate_keeps_the_preparation_card_and_escape_responsive_on_the_real_window() {
    for operation in [
        "prepare_decode",
        "prepare_convert",
        "prepare_prime",
        "prepare_graph",
        "prepare_output_stop",
        "prepare_get_state",
    ] {
        for size in ["1200,800", "880,600"] {
            let root = tempfile::tempdir().unwrap();
            let log = root.path().join("probe.log");
            let output = fs::File::create(&log).unwrap();
            let mut child = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
                .args([
                    "--park", "--silent", "--script", "prepared", "--size", size, "--shot",
                ])
                .arg(root.path())
                .env("XIRIUS_CONFIG_DIR", root.path().join("prefs"))
                .env("XIRIUS_CRASH_DIR", root.path().join("crashes"))
                .env("XIRIUS_PREPARED_SHOTS_DIR", root.path())
                .env("XIRIUS_PREPARED_GATE", operation)
                .stdout(output.try_clone().unwrap())
                .stderr(output)
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(120);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    child.wait().unwrap();
                    panic!(
                        "preparation window watchdog: {}",
                        fs::read_to_string(&log).unwrap()
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let log = fs::read_to_string(log).unwrap();
            assert!(status.success(), "{operation} {size}: {log}");
            for stage in [
                "reading",
                "moved",
                "resized",
                "restored",
                "cancel",
                if operation == "prepare_get_state" {
                    "save-cancelled"
                } else {
                    "closed"
                },
                "failed",
                "detail",
                "detail-end",
                "retried",
                "silent",
            ] {
                assert!(
                    log.contains(&format!("prepared-check stage={stage} ok=true")),
                    "{log}"
                );
            }
            assert!(log.contains("12 saved, 0 failed"), "{operation}: {log}");
            for name in [
                "prepared-00-before",
                "prepared-01a-reading",
                "prepared-01b-moved",
                "prepared-01c-resized",
                "prepared-01d-restored",
                "prepared-02-cancelling",
                "prepared-03-closed",
                "prepared-04a-failed",
                "prepared-04b-detail",
                "prepared-04c-detail-end",
                "prepared-05-retried",
                "prepared-06-silent",
            ] {
                let bmp = fs::read(root.path().join(format!("{name}.bmp"))).unwrap();
                assert!(bmp.len() > 1024 && &bmp[..2] == b"BM", "{name}");
            }
        }
    }
}
