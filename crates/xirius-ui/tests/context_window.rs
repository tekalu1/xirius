//! Approved context menus on the actual parked window. Captures use PrintWindow;
//! input is dispatched only to that window, never to the OS input queue.
mod support;

#[cfg(windows)]
#[test]
fn six_targets_cascade_and_search_in_both_themes() {
    for light in [false, true] {
        let sandbox = tempfile::Builder::new()
            .prefix("xirius-context-window-")
            .tempdir()
            .unwrap();
        let root = sandbox.path();
        let shots = root.join("shots");
        std::fs::create_dir_all(&shots).unwrap();
        let mut command = support::app(root);
        command
            .args([
                "--park",
                "--silent",
                "--size",
                "1440,1000",
                "--script",
                "ctx-v2",
                "--shot",
            ])
            .arg(&shots);
        if light {
            command.arg("--light");
        }
        let log = root.join("stderr.log");
        command.stdout(std::process::Stdio::null());
        command.stderr(std::fs::File::create(&log).unwrap());
        let mut child = command.spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("context capture timed out: light={light}");
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        let stderr = std::fs::read_to_string(log).unwrap();
        assert!(status.success(), "light={light}: {stderr}");
        for target in [
            "audio", "midi", "track", "insert", "note", "empty", "cascade", "search",
        ] {
            let image = shots.join(format!("v2-{target}.bmp"));
            assert!(
                image.metadata().is_ok_and(|m| m.len() > 54),
                "light={light}, {target}: {stderr}"
            );
        }
    }
}
