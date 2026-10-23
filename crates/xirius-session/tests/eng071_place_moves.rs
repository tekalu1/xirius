//! A plugin moves between the app's process and a plugin host process, or between hosts, while
//! a new mix for it is on its way (ENG-071):
//!
//! - until the new mix plays, the plugin is the instance that plays: its window, what it
//!   reports (its latency, its window) and the instance the screen opens a window on;
//! - a window closed while the new mix is on its way does not open on the new instance;
//! - one moved into this process has its host's window shut when the new mix plays (the screen
//!   opens it here once that host has let go of the old instance);
//! - a window the person shuts in the old host just as the new mix plays stays shut, though the
//!   host says so only after the new mix played;
//! - a new mix let go leaves the plugin where it was.
//!
//! Skips when `xirius-plugin-host` has not been built (`cargo test --workspace` builds it).
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_engine::AudioEngine;
use xirius_engine::server::{PluginLink, PluginServer, PluginStatus, find_host};
use xirius_plugins::{Catalog, probe};
use xirius_session::host::{HostConfig, SessionHost, SyncReport};
use xirius_session::plugins::Isolation;

/// How long the test plugin takes to be made, so a new mix stays pending a while.
const OPEN: Duration = Duration::from_millis(900);
const PID: PluginId = PluginId(1);
/// A second plugin, made in a host by the new mix while the first is made in the app.
const OTHER: PluginId = PluginId(2);
const TRACK: TrackId = TrackId(100);

/// The song with the plugin at `isolation`, its delay parameter at `delay` (0..1 of 256).
fn song(uid: &str, isolation: PluginIsolation, delay: f64) -> Project {
    song_with(uid, isolation, delay, None)
}

/// As [`song`], with a second plugin after it at `other`.
fn song_with(
    uid: &str,
    isolation: PluginIsolation,
    delay: f64,
    other: Option<PluginIsolation>,
) -> Project {
    let mut t = Track::new(TRACK, TrackKind::Audio, "T".to_string());
    let plugin = |id, isolation| Plugin {
        samples: Vec::new(),
        id,
        name: "Test".into(),
        kind: PluginKind::Effect,
        format: "vst3".into(),
        uid: uid.to_string(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::from([("1".to_string(), delay)]),
        state: None,
        isolation,
        sidechain: None,
    };
    t.inserts.push(plugin(PID, isolation));
    t.inserts.extend(other.map(|i| plugin(OTHER, i)));
    let mut p = Project::new("place-moves");
    p.root.push(Node::Track(t.id));
    p.tracks.insert(t.id, Arc::new(t));
    p
}

struct Rig {
    uid: String,
    catalog: Arc<Catalog>,
    exe: std::path::PathBuf,
}

impl Rig {
    fn start(&self, isolation: PluginIsolation, delay: f64) -> (SessionHost, AudioEngine) {
        self.start_with(&song(&self.uid, isolation, delay))
    }

    fn start_with(&self, song: &Project) -> (SessionHost, AudioEngine) {
        let (mut host, audio, warnings) = SessionHost::start(
            song,
            HostConfig {
                catalog: Some(self.catalog.clone()),
                isolation: Some(Isolation::PerPlugin(Some(self.exe.clone()))),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        host.set_nonblocking(true);
        (host, audio)
    }
}

fn status(host: &SessionHost) -> Option<PluginStatus> {
    host.plugin_statuses()
        .into_iter()
        .find(|(id, ..)| *id == PID)
        .map(|(.., s)| s)
}

/// Runs the frames of the screen until the new mix plays, calling `during` on each before the
/// step of the swap.
fn swap(
    host: &mut SessionHost,
    audio: &mut AudioEngine,
    mut during: impl FnMut(&mut SessionHost),
) -> SyncReport {
    let deadline = Instant::now() + Duration::from_secs(60);
    let report = loop {
        assert!(Instant::now() < deadline, "the mix never came in");
        during(host);
        if let Some(r) = host.step_swap() {
            break r;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(report.swapped, "{:?}", report.warnings);
    audio.render(&mut [0.0; 512], 2);
    host.controller().poll(|_| {});
    report
}

/// Runs the frames of the screen until the old host has let go of the instance that played:
/// what it said about its window is in by then.
fn settle(host: &mut SessionHost, old_server: &PluginServer, old_link: &PluginLink) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let gone = old_server.has_let_go(old_link);
        host.poll();
        if gone {
            return;
        }
        assert!(Instant::now() < deadline, "the old host never let go");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The plugin's window in the host process `pid`, once it is there.
#[cfg(windows)]
fn host_window(pid: u32) -> isize {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowExW(
            parent: isize,
            after: isize,
            class: *const u16,
            title: *const u16,
        ) -> isize;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
    }
    let class: Vec<u16> = "XiriusPluginEditor".encode_utf16().chain(Some(0)).collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut hwnd = 0;
        loop {
            // SAFETY: the class name is a NUL-terminated string that outlives the call.
            hwnd = unsafe { FindWindowExW(0, hwnd, class.as_ptr(), std::ptr::null()) };
            if hwnd == 0 {
                break;
            }
            let mut of = 0;
            // SAFETY: `of` outlives the call.
            unsafe { GetWindowThreadProcessId(hwnd, &mut of) };
            if of == pid {
                return hwnd;
            }
        }
        assert!(Instant::now() < deadline, "the host's window never came");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Shuts a window as its close box does (a message to it, not a click): returns once the
/// window's process has taken it, before that process has said anything about it.
#[cfg(windows)]
fn close_window(hwnd: isize) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SendMessageTimeoutW(
            hwnd: isize,
            msg: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            timeout: u32,
            result: *mut usize,
        ) -> isize;
    }
    const WM_CLOSE: u32 = 0x0010;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    let mut out = 0;
    // SAFETY: the message carries no pointers; the time limit keeps a hung host from hanging
    // the test.
    let took =
        unsafe { SendMessageTimeoutW(hwnd, WM_CLOSE, 0, 0, SMTO_ABORTIFHUNG, 2000, &mut out) };
    assert_ne!(took, 0, "the host's window did not take the close");
}

#[test]
fn a_plugin_moved_between_processes_is_the_one_that_plays_until_the_new_mix_does() {
    let Some(exe) = find_host() else {
        eprintln!("opening in hosts needs the plugin host binary");
        return;
    };
    // SAFETY: the only test of this binary; the hosts it starts read them.
    unsafe {
        std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", OPEN.as_millis().to_string());
        // The windows open off every screen: nothing comes to the front.
        std::env::set_var(xirius_vst3::OFFSCREEN_ENV, "1");
    }
    xirius_test_vst3::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let entries = probe(&deps.join("xirius_test_vst3.dll")).unwrap();
    let rig = Rig {
        uid: entries[0].uid.clone(),
        catalog: Arc::new(Catalog {
            entries,
            failures: Vec::new(),
        }),
        exe,
    };
    into_the_app(&rig);
    out_of_the_app(&rig);
    for to in [PluginIsolation::Shared, PluginIsolation::InProcess] {
        closed_while_on_its_way(&rig, to, true);
        closed_while_on_its_way(&rig, to, false);
    }
    window_stays_after_it_played(&rig);
    #[cfg(windows)]
    for to in [PluginIsolation::Shared, PluginIsolation::InProcess] {
        shut_as_it_plays(&rig, to);
    }
    let_go(&rig);
}

/// From its own host into the app, with its window open.
fn into_the_app(rig: &Rig) {
    let (mut host, mut audio) = rig.start(PluginIsolation::Dedicated, 0.75);
    let old_server = host.plugins().server_of(PID).unwrap().clone();
    let old_link = host.plugins().link(PID).unwrap();
    let before = status(&host).unwrap().latency;
    host.plugin_editor(PID, true, None).unwrap();

    let p2 = song(&rig.uid, PluginIsolation::InProcess, 0.25);
    assert!(!host.sync_later(&p2).swapped);
    let mut frames = 0;
    swap(&mut host, &mut audio, |host| {
        frames += 1;
        // The host's instance plays on: its window, its latency, its process.
        let s = status(host).expect("the instance that plays is reported");
        assert!(s.editor_open, "its window counts as open");
        assert_eq!(s.latency, before, "the latency of the instance that plays");
        let r = host.plugins().reports();
        let r = r.iter().find(|r| r.id == PID).unwrap();
        assert_eq!(r.placement, PluginIsolation::Dedicated);
        assert!(
            host.plugins().local(PID).is_none(),
            "no window opens on the instance that does not play yet"
        );
    });
    assert!(frames > 1, "the new mix was on its way a while");

    assert!(host.plugins().local(PID).is_some(), "it plays in the app");
    assert!(status(&host).is_none(), "nothing plays it in a host");
    assert!(
        old_link.editor().is_none(),
        "the host's window is shut: the screen opens it here"
    );
    // Once the host has let go of the old instance, with nothing said about its window.
    assert!(host.plugins().window_moving_in(PID));
    assert!(!host.plugins().window_moved_in(PID));
    settle(&mut host, &old_server, &old_link);
    assert!(!host.plugins().window_moving_in(PID));
    assert!(host.plugins().window_moved_in(PID));
    assert_eq!(old_link.windows_shut(), 0, "nobody shut it");
}

/// From the app into its own host.
fn out_of_the_app(rig: &Rig) {
    let (mut host, mut audio) = rig.start(PluginIsolation::InProcess, 0.75);
    let (_, generation) = host.plugins().local(PID).expect("it plays in the app");
    let p2 = song(&rig.uid, PluginIsolation::Dedicated, 0.25);
    assert!(!host.sync_later(&p2).swapped);
    let mut asked = None;
    swap(&mut host, &mut audio, |host| {
        assert_eq!(
            host.plugins().local(PID).map(|(_, g)| g),
            Some(generation),
            "the window stays on the instance that plays"
        );
        assert!(status(host).is_none(), "no host plays it yet");
        // The screen's window is this process's: nothing to open in the host.
        asked = Some(host.plugin_editor(PID, false, None));
    });
    assert!(asked.is_some_and(|r| r.is_err()));
    assert!(host.plugins().local(PID).is_none());
    let s = status(&host).expect("its host plays it");
    assert!(!s.editor_open, "the screen opens the window in the host");
    host.plugin_editor(PID, true, None).unwrap();
    assert!(status(&host).unwrap().editor_open);
}

/// From its own host to the shared one (`to`) or into the app, the window closed while the new
/// mix is on its way: through the screen (`screen`), or by the host saying so (the person
/// closed it there).
fn closed_while_on_its_way(rig: &Rig, to: PluginIsolation, screen: bool) {
    let (mut host, mut audio) = rig.start(PluginIsolation::Dedicated, 0.75);
    let old_server = host.plugins().server_of(PID).unwrap().clone();
    let old_link = host.plugins().link(PID).unwrap();
    let before = status(&host).unwrap().latency;
    let base = before - 192;
    host.plugin_editor(PID, true, None).unwrap();

    let p2 = song(&rig.uid, to, 0.25);
    assert!(!host.sync_later(&p2).swapped);
    let apart = to != PluginIsolation::InProcess;
    let mut frames = 0;
    swap(&mut host, &mut audio, |host| {
        frames += 1;
        let s = status(host).unwrap();
        assert_eq!(s.latency, before, "the latency of the instance that plays");
        // The new instance is asked to open in the first step.
        if frames == 2 {
            assert!(s.editor_open);
            if apart {
                assert!(
                    host.plugins().link(PID).unwrap().editor().is_some(),
                    "the new instance's window is to open when it plays"
                );
            }
            if screen {
                host.plugin_editor(PID, false, None).unwrap();
            } else {
                old_server.close_editor(&old_link);
            }
        }
        if frames > 2 {
            assert!(!s.editor_open, "the window is closed");
        }
    });
    assert!(frames >= 2, "the new mix was on its way a while");

    if apart {
        let link = host.plugins().link(PID).unwrap();
        assert!(!Arc::ptr_eq(&link, &old_link), "a new link in the new host");
        assert!(link.editor().is_none(), "the window does not open again");
        let s = status(&host).unwrap();
        assert!(!s.editor_open);
        assert_eq!(s.latency, base + 64, "the new instance's latency");
    } else {
        assert!(host.plugins().local(PID).is_some(), "it plays in the app");
        assert!(
            !host.plugins().window_moved_in(PID),
            "the window does not open in the app"
        );
    }
}

/// From its own host to the shared one, the window open and nobody shutting it: the new mix
/// plays with the window on the new instance, and it stays there once the old host has let go
/// of the old instance.
fn window_stays_after_it_played(rig: &Rig) {
    let (mut host, mut audio) = rig.start(PluginIsolation::Dedicated, 0.75);
    let old_server = host.plugins().server_of(PID).unwrap().clone();
    let old_link = host.plugins().link(PID).unwrap();
    host.plugin_editor(PID, true, None).unwrap();

    let p2 = song(&rig.uid, PluginIsolation::Shared, 0.25);
    assert!(!host.sync_later(&p2).swapped);
    swap(&mut host, &mut audio, |_| {});

    let link = host.plugins().link(PID).unwrap();
    assert!(!Arc::ptr_eq(&link, &old_link), "a new link in the new host");
    assert!(
        link.editor().is_some(),
        "the window follows to the new host"
    );

    settle(&mut host, &old_server, &old_link);
    assert_eq!(old_link.windows_shut(), 0, "nobody shut it");
    assert!(link.editor().is_some(), "the window stays open");
    assert!(status(&host).unwrap().editor_open);
}

/// From its own host to the shared one (`to`) or into the app, the window open: the person
/// shuts it in the old host right before the step that swaps the new mix in. The old host says
/// so as the mix changes, or as it lets go of the old instance after the new mix played (the
/// word then comes before its answer to the close). The window stays shut: not open on the new
/// instance, nor opened in the app.
///
/// A second plugin moves from its own host to the shared one in the same new mix, so the new
/// mix waits for one made in a host even when the first is made in the app (at once in its
/// step): the window is shut once both are made, before the step that swaps.
#[cfg(windows)]
fn shut_as_it_plays(rig: &Rig, to: PluginIsolation) {
    let dedicated = Some(PluginIsolation::Dedicated);
    let p = song_with(&rig.uid, PluginIsolation::Dedicated, 0.75, dedicated);
    let (mut host, mut audio) = rig.start_with(&p);
    let old_server = host.plugins().server_of(PID).unwrap().clone();
    let old_link = host.plugins().link(PID).unwrap();
    host.plugin_editor(PID, true, None).unwrap();
    let frame = host_window(old_server.pid().unwrap());

    let p2 = song_with(&rig.uid, to, 0.25, Some(PluginIsolation::Shared));
    assert!(!host.sync_later(&p2).swapped);
    let mut frames = 0;
    let mut shut = false;
    swap(&mut host, &mut audio, |host| {
        frames += 1;
        if frames == 2 {
            // The first step asked the shared host to make the second plugin: the ones made
            // from now on are made at once.
            // SAFETY: as in the test's start; the host that reads it has started.
            unsafe { std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", "0") };
        }
        let p = host.plugins();
        if shut || !p.preloaded(PID) || !p.preloaded(OTHER) {
            return;
        }
        // Once both are made, the next step swaps the new mix in.
        let deadline = Instant::now() + Duration::from_secs(10);
        while host.plugins().preparing(PID) || host.plugins().preparing(OTHER) {
            assert!(Instant::now() < deadline, "the new instances never came");
            std::thread::sleep(Duration::from_millis(1));
        }
        close_window(frame);
        shut = true;
    });
    // SAFETY: as above.
    unsafe { std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", OPEN.as_millis().to_string()) };
    assert!(shut, "the window was shut before the new mix played");
    settle(&mut host, &old_server, &old_link);
    assert_eq!(old_link.windows_shut(), 1, "the old host said it was shut");
    if to == PluginIsolation::InProcess {
        assert!(host.plugins().local(PID).is_some(), "it plays in the app");
        assert!(
            !host.plugins().window_moved_in(PID),
            "the window does not open in the app"
        );
    } else {
        let link = host.plugins().link(PID).unwrap();
        assert!(!Arc::ptr_eq(&link, &old_link), "a new link in the new host");
        assert!(link.editor().is_none(), "the window does not stay open");
        assert!(!status(&host).unwrap().editor_open);
    }
}

/// A new mix that moves it to the shared host, let go while the new instance opens: it stays
/// in its own host, with its window.
fn let_go(rig: &Rig) {
    let p = song(&rig.uid, PluginIsolation::Dedicated, 0.75);
    let (mut host, _audio) = rig.start(PluginIsolation::Dedicated, 0.75);
    let old_link = host.plugins().link(PID).unwrap();
    let old_pid = host.plugins().server_of(PID).unwrap().pid();
    host.plugin_editor(PID, true, None).unwrap();

    let p2 = song(&rig.uid, PluginIsolation::Shared, 0.25);
    assert!(!host.sync_later(&p2).swapped);
    assert!(host.step_swap().is_none(), "the new instance opens");
    let new_link = host.plugins().link(PID).unwrap();
    assert!(!Arc::ptr_eq(&new_link, &old_link));
    assert!(host.swap_pending());
    // The song is back to what plays.
    let report = host.sync(&p);
    assert!(!report.swapped, "nothing new to build");
    assert!(!host.swap_pending());

    let link = host.plugins().link(PID).expect("it plays in its host");
    assert!(Arc::ptr_eq(&link, &old_link), "the instance that played");
    assert_eq!(host.plugins().server_of(PID).unwrap().pid(), old_pid);
    assert!(host.plugins().local(PID).is_none());
    assert!(status(&host).unwrap().editor_open, "its window stays open");
    let r = host.plugins().reports();
    assert_eq!(
        r.iter().find(|r| r.id == PID).unwrap().placement,
        PluginIsolation::Dedicated
    );
}
