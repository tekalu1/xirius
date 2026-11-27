//! UI-184: on a real (parked, silent) window, what the window hands to the system as the
//! pointer is the tool's own picture over a part, the system's "not allowed" where the tool has
//! nothing to act on, and the system's crosshair when the setting says so.
mod support;

#[cfg(windows)]
#[test]
fn the_window_hands_over_the_tools_pointer() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-pointer-shots-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("pointer-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let mut command = support::app(dir_tmp.path());
    command
        .args(["--park", "--silent", "--script", "pointer", "--shot"])
        .arg(&dir);
    let log = dir.join("stderr.log");
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
            panic!("pointer capture timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    let stderr = std::fs::read_to_string(log).unwrap();
    assert!(status.success(), "{stderr}");
    let checks: Vec<_> = stderr
        .lines()
        .filter(|l| l.starts_with("pointer-check "))
        .collect();
    assert_eq!(checks.len(), 15, "{stderr}");
    for line in checks {
        println!("{line}");
        assert!(line.ends_with("ok=true"), "{line}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
