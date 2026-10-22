//! The program is a GUI program on Windows (UI-290): double-clicking it opens no console. What
//! it prints (--help, --version, --list, a usage line) still reaches a pipe or a file.
#![cfg(windows)]

use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_xirius-ui");

#[test]
fn the_exe_is_marked_as_a_windows_gui_program() {
    let b = std::fs::read(EXE).unwrap();
    let le32 = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
    let pe = le32(0x3c);
    assert_eq!(&b[pe..pe + 4], b"PE\0\0");
    // The optional header follows the 20-byte file header; the subsystem is at offset 68.
    let subsystem = u16::from_le_bytes(b[pe + 24 + 68..pe + 24 + 70].try_into().unwrap());
    assert_eq!(
        subsystem, 2,
        "IMAGE_SUBSYSTEM_WINDOWS_GUI is 2, console is 3"
    );
}

#[test]
fn what_it_prints_still_reaches_a_pipe() {
    let help = Command::new(EXE).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("usage: xirius-ui"));

    let version = Command::new(EXE).arg("--version").output().unwrap();
    let text = String::from_utf8_lossy(&version.stdout);
    assert_eq!(
        text.trim(),
        format!("xirius-ui {}", env!("CARGO_PKG_VERSION"))
    );

    let list = Command::new(EXE)
        .args(["--bench", "--list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    assert!(!list.stdout.is_empty());

    let wrong = Command::new(EXE).arg("--no-such-flag").output().unwrap();
    assert_eq!(wrong.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("usage: xirius-ui"));
}
