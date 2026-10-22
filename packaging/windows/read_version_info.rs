//! Reads the version information of an exe as Explorer does (LIC-022), for the tests of the
//! crates whose build script includes `version_info.rs`:
//! `#[path = "../../../packaging/windows/read_version_info.rs"] mod read_version_info;`

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};

/// What the version information of an exe says.
#[derive(Debug)]
pub struct VersionInfo {
    /// FILEVERSION and PRODUCTVERSION, four numbers each.
    pub file: [u16; 4],
    pub product: [u16; 4],
    /// Whether the file is marked as a pre-release.
    pub prerelease: bool,
    /// The strings of the first language the file lists, by name (`CompanyName`, …).
    pub strings: BTreeMap<String, String>,
}

pub const NAMES: [&str; 8] = [
    "CompanyName",
    "FileDescription",
    "FileVersion",
    "InternalName",
    "LegalCopyright",
    "OriginalFilename",
    "ProductName",
    "ProductVersion",
];

fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain([0]).collect()
}

/// The version information of `exe`, or `None` when it has none.
pub fn read(exe: &Path) -> Option<VersionInfo> {
    let path = wide(exe);
    let mut handle = 0;
    // SAFETY: `path` ends with a 0; the buffer is as long as the size asked for.
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut handle) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    // `unit` is the size of what the length counts: bytes for the blocks, UTF-16 characters
    // (with the ending 0) for the strings.
    let query = |sub: &str, unit: usize| -> Option<&[u8]> {
        let sub = wide(sub);
        let mut at: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        // SAFETY: `at` points into `data`, which outlives the slice.
        let ok = unsafe { VerQueryValueW(data.as_ptr().cast(), sub.as_ptr(), &mut at, &mut len) };
        (ok != 0 && !at.is_null())
            .then(|| unsafe { std::slice::from_raw_parts(at.cast::<u8>(), len as usize * unit) })
    };
    let fixed = query("\\", 1)?;
    assert!(fixed.len() >= std::mem::size_of::<VS_FIXEDFILEINFO>());
    // SAFETY: the root block is a VS_FIXEDFILEINFO.
    let fixed = unsafe { std::ptr::read_unaligned(fixed.as_ptr().cast::<VS_FIXEDFILEINFO>()) };
    let four = |ms: u32, ls: u32| [(ms >> 16) as u16, ms as u16, (ls >> 16) as u16, ls as u16];
    let translation = query("\\VarFileInfo\\Translation", 1)?;
    let language = u16::from_le_bytes([translation[0], translation[1]]);
    let codepage = u16::from_le_bytes([translation[2], translation[3]]);
    let mut strings = BTreeMap::new();
    for name in NAMES {
        let sub = format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\{name}");
        if let Some(value) = query(&sub, 2) {
            let units: Vec<u16> = value
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| u16::from_le_bytes(c))
                .take_while(|&u| u != 0)
                .collect();
            strings.insert(name.to_string(), String::from_utf16(&units).unwrap());
        }
    }
    Some(VersionInfo {
        file: four(fixed.dwFileVersionMS, fixed.dwFileVersionLS),
        product: four(fixed.dwProductVersionMS, fixed.dwProductVersionLS),
        prerelease: fixed.dwFileFlags & 0x2 != 0,
        strings,
    })
}

/// Checks the version information of the program `bin` that the crate built: the version is
/// the crate's (the workspace's), the publisher and the copyright are LICENSE-MIT's, and the
/// description is `description`.
pub fn check(exe: &Path, bin: &str, description: &str) {
    let info = read(exe).unwrap_or_else(|| panic!("{} has no version information", exe.display()));
    let version = env!("CARGO_PKG_VERSION");
    let core = version.split(['-', '+']).next().unwrap();
    let mut numbers: Vec<u16> = core.split('.').map(|n| n.parse().unwrap()).collect();
    numbers.resize(4, 0);
    assert_eq!(info.file.to_vec(), numbers, "{bin}");
    assert_eq!(info.product.to_vec(), numbers, "{bin}");
    assert_eq!(info.prerelease, version.contains('-'), "{bin}");
    let dots = numbers
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(".");
    let file = format!("{bin}.exe");
    let expected = BTreeMap::from(
        [
            ("CompanyName", "The Xirius Authors"),
            ("FileDescription", description),
            ("FileVersion", dots.as_str()),
            ("InternalName", bin),
            ("LegalCopyright", "Copyright (c) 2026 The Xirius Authors"),
            ("OriginalFilename", file.as_str()),
            ("ProductName", "Xirius"),
            ("ProductVersion", version),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    );
    assert_eq!(info.strings, expected, "{bin}");
    // The copyright is the licence's.
    let mit =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../LICENSE-MIT"))
            .unwrap();
    assert!(mit.lines().any(|l| l == info.strings["LegalCopyright"]));
}
