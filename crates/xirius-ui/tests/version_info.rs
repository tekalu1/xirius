//! The version information of the program (LIC-022): the name, the version, the publisher and
//! the copyright that Explorer's 「詳細」 and the Task Manager show, and the icon of the window.
#![cfg(windows)]

#[path = "../../../packaging/windows/read_version_info.rs"]
mod read_version_info;

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW,
    LoadResource, LockResource, SizeofResource,
};

const EXE: &str = env!("CARGO_BIN_EXE_xirius-ui");

#[test]
fn the_program_says_what_it_is_its_version_publisher_and_copyright() {
    read_version_info::check(EXE.as_ref(), "xirius-ui", "Xirius");
}

/// The icon group resource 1 of `exe`, as its bytes.
fn icon_group(exe: &Path) -> Option<Vec<u8>> {
    const RT_GROUP_ICON: u16 = 14;
    let path: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `path` ends with a 0; the module is loaded as data and freed before returning,
    // after the bytes are copied.
    unsafe {
        let module = LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        );
        assert!(!module.is_null(), "{}", exe.display());
        let found = FindResourceW(module, 1 as _, RT_GROUP_ICON as _);
        let bytes = (!found.is_null()).then(|| {
            let data = LockResource(LoadResource(module, found));
            let len = SizeofResource(module, found) as usize;
            std::slice::from_raw_parts(data.cast::<u8>(), len).to_vec()
        });
        FreeLibrary(module);
        bytes
    }
}

#[test]
fn the_first_icon_is_the_brands_which_the_window_takes() {
    let group = icon_group(EXE.as_ref()).expect("the exe has icon 1");
    let ico =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/brand/xirius.ico"))
            .unwrap();
    // Both start with the same header (reserved, type 1, the number of images), and list each
    // image with the same width, height, colours, bits and size; the group names the image by
    // its resource id (2 bytes) where the file gives its offset (4 bytes). The planes, which the
    // file leaves 0, the resource compiler writes as 1.
    assert_eq!(group[..6], ico[..6]);
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    assert!(count > 0);
    assert_eq!(group.len(), 6 + 14 * count);
    for i in 0..count {
        let g = &group[6 + 14 * i..][..12];
        let f = &ico[6 + 16 * i..][..12];
        assert_eq!((&g[..4], &g[6..]), (&f[..4], &f[6..]), "image {i}");
    }
}
