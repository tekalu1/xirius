//! Optional, device-free investigation of the installed Trash 2 exception filter.
use std::sync::Arc;
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
    SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};
use xirius_vst3::{Module, Plugin};

unsafe extern "system" fn observer(_: *const EXCEPTION_POINTERS) -> i32 {
    EXCEPTION_CONTINUE_SEARCH
}

fn inspect(stage: &str) {
    // SAFETY: temporarily reinstalls this test's filter, inspecting the previous pointer.
    unsafe {
        let filter = SetUnhandledExceptionFilter(Some(observer));
        let mut owner = std::ptr::null_mut();
        let address = filter.map_or(0, |f| f as usize);
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address as *const u16,
            &mut owner,
        );
        let mut name = [0u16; 1024];
        let len = if owner.is_null() {
            0
        } else {
            GetModuleFileNameW(owner, name.as_mut_ptr(), name.len() as u32)
        };
        eprintln!(
            "{stage}: replaced={} owner={}",
            address != observer as *const () as usize,
            String::from_utf16_lossy(&name[..len as usize])
        );
    }
}

#[test]
#[ignore = "requires installed iZotope Trash 2; run alone, no editor or audio devices"]
fn trash_2_filter_probe() {
    // SAFETY: only this diagnostic test process changes its exception settings.
    unsafe {
        SetErrorMode(SEM_NOGPFAULTERRORBOX);
        SetUnhandledExceptionFilter(Some(observer));
    }
    let path =
        std::path::Path::new(r"C:\Program Files\Common Files\VST3\iZotope\iZotope Trash 2.vst3");
    let module = Arc::new(Module::load(path).unwrap());
    inspect("Module::load");
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .unwrap();
    let plugin = Plugin::new(module, &class).unwrap();
    inspect("Plugin::new");
    let _ = plugin.get_state().unwrap();
    inspect("Plugin::state");
    drop(plugin);
    inspect("Plugin::drop");
}
