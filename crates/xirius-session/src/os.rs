//! What the list of exports asks of the OS: show a file in its file window, and move a file to
//! the recycle bin (EXP-024); and open a file or a web page as the OS opens it (LIC-015).

use std::path::Path;
use std::process::Command;

/// Opens the OS's file window on `path`, with the file picked where the OS can.
pub fn reveal(path: &Path) -> Result<(), String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    let mut cmd = reveal_command(&path);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("ファイルの場所を開けませんでした: {e}"))
}

/// The OSes the file window is asked of differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    Windows,
    Mac,
    Other,
}

impl Desktop {
    pub const fn current() -> Desktop {
        if cfg!(windows) {
            Desktop::Windows
        } else if cfg!(target_os = "macos") {
            Desktop::Mac
        } else {
            Desktop::Other
        }
    }
}

/// The program and the arguments that open the file window on `path` (an absolute path) on
/// `desktop`. Windows takes one argument that is passed as it is, quotes and all.
pub fn reveal_launch(desktop: Desktop, path: &Path) -> (&'static str, Vec<String>) {
    match desktop {
        Desktop::Windows => (
            "explorer.exe",
            vec![format!("/select,\"{}\"", path.display())],
        ),
        Desktop::Mac => ("open", vec!["-R".into(), path.display().to_string()]),
        Desktop::Other => (
            "xdg-open",
            vec![path.parent().unwrap_or(path).display().to_string()],
        ),
    }
}

fn reveal_command(path: &Path) -> Command {
    let (program, args) = reveal_launch(Desktop::current(), path);
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        for a in &args {
            c.raw_arg(a);
        }
    }
    #[cfg(not(windows))]
    c.args(&args);
    c
}

/// Opens a file, or a web page (`https://…`), with what the OS opens it with.
pub fn open(target: &str) -> Result<(), String> {
    open_command(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{target} を開けませんでした: {e}"))
}

#[cfg(windows)]
fn open_command(target: &str) -> Command {
    let mut c = Command::new("explorer.exe");
    c.arg(target);
    c
}

#[cfg(target_os = "macos")]
fn open_command(target: &str) -> Command {
    let mut c = Command::new("open");
    c.arg(target);
    c
}

#[cfg(not(any(windows, target_os = "macos")))]
fn open_command(target: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(target);
    c
}

/// The shell's apartment on a thread of its own, as the screen's thread has: [`trash`] runs the
/// shell's extensions (a cloud drive's), which may need one. Let go when dropped, on that thread.
#[must_use]
pub struct Apartment {
    #[cfg_attr(not(windows), allow(dead_code))]
    joined: bool,
    _here: std::marker::PhantomData<*const ()>,
}

/// Joins this thread to an apartment of its own (nothing where the OS has none).
pub fn apartment() -> Apartment {
    #[cfg(windows)]
    let joined = {
        use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: no reserved pointer; let go by `Apartment`'s drop on this thread, as the
        // `PhantomData` keeps it here.
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) >= 0 }
    };
    #[cfg(not(windows))]
    let joined = false;
    Apartment {
        joined,
        _here: std::marker::PhantomData,
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.joined {
            // SAFETY: matches the `CoInitializeEx` that succeeded on this thread.
            unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        }
    }
}

/// Moves `path` to the recycle bin, so the person can take it back.
pub fn trash(path: &Path) -> Result<(), String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    to_trash(&path).map_err(|e| format!("{} をごみ箱に移動できませんでした: {e}", path.display()))
}

#[cfg(windows)]
fn to_trash(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };
    // A list of paths, each ending in a 0, the list in another.
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as _,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `op` points at `from`, a list ending in two 0s that outlives the call; no window
    // is shown and nothing is written back but the fields of `op`.
    let r = unsafe { SHFileOperationW(&mut op) };
    match (r, op.fAnyOperationsAborted) {
        (0, 0) => Ok(()),
        (0, _) => Err("キャンセルされました".into()),
        (code, _) => Err(format!("SHFileOperation がエラー {code:#x} を返しました")),
    }
}

#[cfg(target_os = "macos")]
fn to_trash(path: &Path) -> Result<(), String> {
    let script = format!(
        "tell application \"Finder\" to delete POSIX file \"{}\"",
        path.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    );
    let out = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn to_trash(path: &Path) -> Result<(), String> {
    let out = Command::new("gio")
        .arg("trash")
        .arg(path)
        .output()
        .map_err(|e| format!("gio が見つかりません: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Renames `from` to `to` on the same drive unless there is a file `to`: then an error of
/// [`std::io::ErrorKind::AlreadyExists`], and nothing moves. A file another program makes
/// with that name just before is never written over (REC-026).
pub fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
    rename_no_replace(from, to)
}

#[cfg(windows)]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both paths end in a 0 and outlive the call. Without `MOVEFILE_REPLACE_EXISTING`
    // it fails with `ERROR_ALREADY_EXISTS` when `to` is there.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| {
        std::ffi::CString::new(p.as_os_str().as_bytes())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
    };
    let (f, t) = (c(from)?, c(to)?);
    // SAFETY: both paths end in a 0 and outlive the call.
    #[cfg(target_os = "linux")]
    let r = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            f.as_ptr(),
            libc::AT_FDCWD,
            t.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    // SAFETY: as above.
    #[cfg(target_os = "macos")]
    let r = unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) };
    if r == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        // A drive (or a kernel) that cannot tell: a link, which is never made over a file.
        Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP) => link_and_remove(from, to),
        _ => Err(e),
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    link_and_remove(from, to)
}

#[cfg(not(windows))]
fn link_and_remove(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::hard_link(from, to)?;
    std::fs::remove_file(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_renamed_over_none() {
        let dir = tempfile::Builder::new()
            .prefix("xirius-os-rename-")
            .tempdir()
            .unwrap();
        let (a, b, c) = (
            dir.path().join("a.wav"),
            dir.path().join("b.wav"),
            dir.path().join("c.wav"),
        );
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let e = rename_new(&a, &b).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists, "{e}");
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        rename_new(&a, &c).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read(&c).unwrap(), b"a");
    }

    #[test]
    fn each_os_is_asked_for_the_file_in_its_own_way_and_nothing_is_launched() {
        let p = Path::new("/music/My Song/kick.wav");
        assert_eq!(
            reveal_launch(Desktop::Mac, p),
            ("open", vec!["-R".to_string(), p.display().to_string()])
        );
        assert_eq!(
            reveal_launch(Desktop::Other, p),
            ("xdg-open", vec!["/music/My Song".to_string()])
        );
        let w = Path::new(r"D:\music\My Song\kick.wav");
        assert_eq!(
            reveal_launch(Desktop::Windows, w),
            (
                "explorer.exe",
                vec![r#"/select,"D:\music\My Song\kick.wav""#.to_string()]
            ),
            "one argument, the path in quotes, so a space does not split it"
        );
    }

    #[test]
    fn the_command_of_this_os_carries_those_arguments() {
        let p = std::path::absolute("kick.wav").unwrap();
        let (program, args) = reveal_launch(Desktop::current(), &p);
        let c = reveal_command(&p);
        assert_eq!(c.get_program(), program);
        let got: Vec<String> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(got, args);
    }
}
