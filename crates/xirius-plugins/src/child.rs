//! The process that opens one bundle and writes what it offers to a file.
//!
//! `<program> [prefix] --bundle <path> --out <file> [--crash-on <text>] [--hang-on <text>]`
//!
//! It answers with `{"entries": [...]}` or `{"error": "..."}`. Ending without writing the file
//! means the bundle crashed it. `--crash-on` and `--hang-on` make it crash or hang when the
//! bundle path contains the text, for tests.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use xirius_core::model::PluginKind;

use crate::{Entry, uid_hex};

/// The argument the `xirius` command takes to act as the scan process.
pub const CHILD_ARG: &str = "__scan-plugin";

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Answer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<Entry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Opens `bundle` in this process and lists its audio classes. A plugin that crashes takes
/// the caller with it; [`crate::scan`] calls this in a process of its own.
pub fn probe(bundle: &Path) -> Result<Vec<Entry>, String> {
    if xirius_clap::is_clap(bundle) {
        return crate::clap::probe(bundle);
    }
    let module = xirius_vst3::Module::load(bundle).map_err(|e| e.to_string())?;
    let classes = module.classes();
    let entries = classes
        .iter()
        .filter(|c| c.is_audio_processor())
        .map(|c| Entry {
            uid: uid_hex(&c.cid),
            kind: if c.is_instrument() {
                PluginKind::Instrument
            } else {
                PluginKind::Effect
            },
            vendor: if c.vendor.is_empty() {
                module.vendor.clone()
            } else {
                c.vendor.clone()
            },
            name: c.name.clone(),
            format: "vst3".into(),
            category: c.sub_categories.clone(),
            version: c.version.clone(),
            path: bundle.to_path_buf(),
            ara: crate::ara::uses_ara(&classes, c),
            requires_process_main: false,
        })
        .collect();
    // Some modules crash when unloaded; the answer is what matters and the process ends.
    std::mem::forget(module);
    Ok(entries)
}

/// The scan process. `args` are the arguments after the program (and after [`CHILD_ARG`]).
/// Once the answer is written it ends the process itself; otherwise it returns the exit code.
pub fn child_main(args: &[String]) -> i32 {
    crate::spawn::started();
    quiet_crashes();
    let mut bundle: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let (mut crash_on, mut hang_on) = (None::<String>, None::<String>);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().cloned();
        match (a.as_str(), v) {
            ("--bundle", Some(v)) => bundle = Some(v.into()),
            ("--out", Some(v)) => out = Some(v.into()),
            ("--crash-on", Some(v)) => crash_on = Some(v),
            ("--hang-on", Some(v)) => hang_on = Some(v),
            _ => {
                eprintln!("usage: --bundle <path> --out <file>");
                return 2;
            }
        }
    }
    let (Some(bundle), Some(out)) = (bundle, out) else {
        eprintln!("usage: --bundle <path> --out <file>");
        return 2;
    };
    let text = bundle.to_string_lossy();
    if crash_on.is_some_and(|c| text.contains(&c)) {
        crash();
    }
    if hang_on.is_some_and(|h| text.contains(&h)) {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }
    let answer = match probe(&bundle) {
        Ok(entries) => Answer {
            entries: Some(entries),
            error: None,
        },
        Err(e) => Answer {
            entries: None,
            error: Some(e),
        },
    };
    let body = serde_json::to_vec(&answer).expect("an answer serialises");
    match std::fs::write(&out, body) {
        Ok(()) => end_now(0),
        Err(e) => {
            eprintln!("{}: {e}", out.display());
            3
        }
    }
}

/// Ends the process without letting the loaded DLLs run their unload code: some plugins
/// (seen with Native Instruments and Plugin Alliance bundles) hang there after the answer is
/// written, and the scan would wait out its whole timeout for each.
pub(crate) fn end_now(code: u32) -> i32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        // SAFETY: ends this process; the answer file is already closed.
        unsafe { TerminateProcess(GetCurrentProcess(), code) };
    }
    code as i32
}

/// A crash must not put a dialog in front of the user; the scan notices it by itself.
pub(crate) fn quiet_crashes() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
        };
        use windows_sys::Win32::System::ErrorReporting::{WER_FAULT_REPORTING_NO_UI, WerSetFlags};
        // SAFETY: no preconditions.
        unsafe {
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
            WerSetFlags(WER_FAULT_REPORTING_NO_UI);
        }
    }
}

/// What a plugin that dereferences a bad pointer does to its host.
fn crash() -> ! {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::EXCEPTION_ACCESS_VIOLATION;
        use windows_sys::Win32::System::Diagnostics::Debug::RaiseException;
        // SAFETY: raising a non-continuable exception ends the process, which is the point.
        unsafe { RaiseException(EXCEPTION_ACCESS_VIOLATION as u32, 1, 0, std::ptr::null()) };
    }
    std::process::abort()
}
