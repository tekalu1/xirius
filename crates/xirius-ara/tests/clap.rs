//! PLG-033: a CLAP plugin bound to the document with roles, through `ARACLAP.h`.
//!
//! The CLAP plugin here is a stand-in written against CLAP's C API: its entry lists the fake ARA
//! factory under the ARA factory id, and its instances answer the ARA plugin extension by
//! binding the fake's renderer. The host side under test is `xirius_ara::clap`.

use std::ffi::{CStr, c_char, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::CLAP_TRANSPORT_IS_PLAYING;
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::fixedpoint::CLAP_SECTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{CLAP_PROCESS_CONTINUE, clap_process, clap_process_status};
use clap_sys::version::CLAP_VERSION;

use xirius_ara::clap::{
    ARA_EXTENSION, ARA_FACTORY, ClapBinary, clap_ara_factory, clap_ara_plugin_extension,
};
use xirius_ara::fake::{self, Config};
use xirius_ara::service::{Binary, Instance, Loader, source_id};
use xirius_ara::sys::{self, Ref, RoleFlags};
use xirius_ara::{ClipSpec, Phase, Service, Song, SourceAudio};

const PLUGIN_ID: &CStr = c"org.xirius.fake.clap";

struct Shared<T>(T);
// SAFETY: the wrapped structs point at static, immutable data only.
unsafe impl<T> Sync for Shared<T> {}

/// What the host asked of the stand-in.
static BOUND: AtomicU32 = AtomicU32::new(0);
static ROLES: AtomicI32 = AtomicI32::new(0);
static PROCESSED: AtomicU32 = AtomicU32::new(0);

// The ARA factory list -----------------------------------------------------------------------

unsafe extern "C" fn factory_count(_f: *const clap_ara_factory) -> u32 {
    1
}
unsafe extern "C" fn ara_factory(_f: *const clap_ara_factory, _i: u32) -> *const sys::Factory {
    fake::factory()
}
unsafe extern "C" fn ara_plugin_id(_f: *const clap_ara_factory, _i: u32) -> *const c_char {
    PLUGIN_ID.as_ptr()
}
static ARA_FACTORIES: clap_ara_factory = clap_ara_factory {
    get_factory_count: factory_count,
    get_ara_factory: ara_factory,
    get_plugin_id: ara_plugin_id,
};

// The plugin ---------------------------------------------------------------------------------

/// A plugin's own data: the fake's renderer once bound, and the rate it was activated at.
struct Data {
    inner: Option<Box<dyn Instance>>,
    rate: f64,
}

fn data(p: *const clap_plugin) -> &'static mut Data {
    // SAFETY: every plugin here is made by `create` with a boxed `Data`.
    unsafe { &mut *((*p).plugin_data as *mut Data) }
}

unsafe extern "C" fn init(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn destroy(p: *const clap_plugin) {
    // SAFETY: made by `create` from these two boxes; destroyed once.
    unsafe {
        drop(Box::from_raw((*p).plugin_data as *mut Data));
        drop(Box::from_raw(p as *mut clap_plugin));
    }
}
unsafe extern "C" fn activate(p: *const clap_plugin, rate: f64, _min: u32, max: u32) -> bool {
    let d = data(p);
    d.rate = rate;
    d.inner
        .as_mut()
        .is_some_and(|i| i.prepare(rate, max as usize).is_ok())
}
unsafe extern "C" fn deactivate(_p: *const clap_plugin) {}
unsafe extern "C" fn start(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn stop(_p: *const clap_plugin) {}
unsafe extern "C" fn reset(_p: *const clap_plugin) {}
unsafe extern "C" fn process(
    p: *const clap_plugin,
    pr: *const clap_process,
) -> clap_process_status {
    let d = data(p);
    let Some(inner) = d.inner.as_mut() else {
        return CLAP_PROCESS_CONTINUE;
    };
    // SAFETY: the host's process call: one stereo output and a transport, live for the call.
    unsafe {
        let pr = &*pr;
        let t = &*pr.transport;
        let at = t.song_pos_seconds as f64 / CLAP_SECTIME_FACTOR as f64;
        inner.set_transport(
            t.flags & CLAP_TRANSPORT_IS_PLAYING != 0,
            (at * d.rate).round() as i64,
        );
        let out = &*pr.audio_outputs;
        let n = pr.frames_count as usize;
        let l = std::slice::from_raw_parts_mut(*out.data32, n);
        let r = std::slice::from_raw_parts_mut(*out.data32.add(1), n);
        inner.process(l, r);
    }
    PROCESSED.fetch_add(1, Ordering::AcqRel);
    CLAP_PROCESS_CONTINUE
}
unsafe extern "C" fn ext_factory(_p: *const clap_plugin) -> *const sys::Factory {
    fake::factory()
}
unsafe extern "C" fn bind(
    p: *const clap_plugin,
    dc: Ref,
    known: RoleFlags,
    assigned: RoleFlags,
) -> *const sys::PlugInExtensionInstance {
    let Ok(mut binary) = fake::loader(Config::default())() else {
        return std::ptr::null();
    };
    let Ok(inner) = binary.instance(dc, known, assigned) else {
        return std::ptr::null();
    };
    BOUND.fetch_add(1, Ordering::AcqRel);
    ROLES.store(assigned, Ordering::Release);
    let d = data(p);
    // The extension instance lives as long as the plugin: kept beside the renderer.
    let ext = Box::leak(Box::new(inner.extension()));
    d.inner = Some(inner);
    ext
}
static EXTENSION: clap_ara_plugin_extension = clap_ara_plugin_extension {
    get_factory: ext_factory,
    bind_to_document_controller: bind,
};
unsafe extern "C" fn get_extension(_p: *const clap_plugin, id: *const c_char) -> *const c_void {
    // SAFETY: CLAP passes a NUL-terminated id.
    let id = unsafe { CStr::from_ptr(id) };
    if id == ARA_EXTENSION {
        &EXTENSION as *const clap_ara_plugin_extension as *const c_void
    } else {
        std::ptr::null()
    }
}
unsafe extern "C" fn on_main_thread(_p: *const clap_plugin) {}

static FEATURES: Shared<[*const c_char; 3]> = Shared([
    c"audio-effect".as_ptr(),
    c"ara:required".as_ptr(),
    std::ptr::null(),
]);
static DESCRIPTOR: Shared<clap_plugin_descriptor> = Shared(clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: PLUGIN_ID.as_ptr(),
    name: c"Fake ARA (CLAP)".as_ptr(),
    vendor: c"Xirius".as_ptr(),
    url: c"".as_ptr(),
    manual_url: c"".as_ptr(),
    support_url: c"".as_ptr(),
    version: c"1.0.0".as_ptr(),
    description: c"".as_ptr(),
    features: &FEATURES.0 as *const [*const c_char; 3] as *const *const c_char,
});

unsafe extern "C" fn count(_f: *const clap_plugin_factory) -> u32 {
    1
}
unsafe extern "C" fn descriptor(
    _f: *const clap_plugin_factory,
    _i: u32,
) -> *const clap_plugin_descriptor {
    &DESCRIPTOR.0
}
unsafe extern "C" fn create(
    _f: *const clap_plugin_factory,
    _host: *const clap_host,
    id: *const c_char,
) -> *const clap_plugin {
    // SAFETY: CLAP passes a NUL-terminated id.
    if unsafe { CStr::from_ptr(id) } != PLUGIN_ID {
        return std::ptr::null();
    }
    let d = Box::into_raw(Box::new(Data {
        inner: None,
        rate: 0.0,
    }));
    Box::into_raw(Box::new(clap_plugin {
        desc: &DESCRIPTOR.0,
        plugin_data: d as *mut c_void,
        init: Some(init),
        destroy: Some(destroy),
        activate: Some(activate),
        deactivate: Some(deactivate),
        start_processing: Some(start),
        stop_processing: Some(stop),
        reset: Some(reset),
        process: Some(process),
        get_extension: Some(get_extension),
        on_main_thread: Some(on_main_thread),
    }))
}
static PLUGINS: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(count),
    get_plugin_descriptor: Some(descriptor),
    create_plugin: Some(create),
};

// The entry ----------------------------------------------------------------------------------

unsafe extern "C" fn entry_init(_path: *const c_char) -> bool {
    true
}
unsafe extern "C" fn entry_deinit() {}
unsafe extern "C" fn get_factory(id: *const c_char) -> *const c_void {
    // SAFETY: CLAP passes a NUL-terminated id.
    let id = unsafe { CStr::from_ptr(id) };
    if id == ARA_FACTORY {
        &ARA_FACTORIES as *const clap_ara_factory as *const c_void
    } else if id == CLAP_PLUGIN_FACTORY_ID {
        &PLUGINS as *const clap_plugin_factory as *const c_void
    } else {
        std::ptr::null()
    }
}
static ENTRY: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(get_factory),
};

fn loader() -> Loader {
    Box::new(|| {
        // SAFETY: a static entry, initialized by nothing and valid for the process.
        let b = unsafe { ClapBinary::from_entry(&ENTRY, "org.xirius.fake.clap") }?;
        Ok(Box::new(b) as Box<dyn Binary>)
    })
}

#[test]
fn a_clap_plugin_binds_to_the_document_and_plays_the_region() {
    let rate = 48000.0;
    let audio = Arc::new(SourceAudio::stereo(
        rate,
        vec![0.25; 48000],
        vec![0.5; 48000],
    ));
    let s = Service::start(loader(), None, Arc::new(|| {}));
    s.sync(&Song {
        rate,
        clips: vec![ClipSpec {
            key: 1,
            clone_of: None,
            source: source_id("clap.wav"),
            name: "clip".into(),
            audio,
            start: 0.0,
            offset: 0.25,
            duration: 0.5,
        }],
        ..Song::default()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let r = loop {
        if let Some(why) = s.crashed().or(s.failed()) {
            panic!("the plugin stopped: {why}");
        }
        if let Some(r) = s
            .state(1)
            .filter(|c| c.phase == Phase::Ready)
            .and_then(|c| c.rendered)
        {
            break r;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the render"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(s.info().unwrap().factory_id, "org.xirius.fake");
    assert!(BOUND.load(Ordering::Acquire) >= 1);
    assert_ne!(
        ROLES.load(Ordering::Acquire) & sys::ROLE_PLAYBACK_RENDERER,
        0,
        "bound without the playback renderer's role"
    );
    assert!(PROCESSED.load(Ordering::Acquire) > 0);
    assert_eq!(r.left.len(), (0.5 * rate) as usize);
    assert!(r.left.iter().all(|v| (v - 0.25).abs() < 1e-6));
    assert!(r.right.iter().all(|v| (v - 0.5).abs() < 1e-6));
    drop(s);
}
