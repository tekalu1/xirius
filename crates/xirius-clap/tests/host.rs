//! The CLAP host against `xirius-test-clap` (PLG-028): what the bundle offers, parameters,
//! state saved and restored into a new instance, and `process` with notes, parameter changes
//! and an editor's gesture. No window and no device.

use std::path::PathBuf;

use xirius_clap::{Bundle, Gesture, Note, ParamChange, Plugin, class_key};
use xirius_test_clap::{
    EDIT_GAIN, EDIT_KEY, GAIN, ID, LATENCY, MAX_LATENCY, NAME, NOTE_NAMES, SOFT_KICK,
};

const BLOCK: usize = 64;

fn bundle() -> Bundle {
    xirius_test_clap::fixture_marker();
    let deps: PathBuf = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = deps.join("xirius_test_clap.dll");
    assert!(dll.exists(), "the fixture's cdylib is built with the tests");
    Bundle::load(&dll).expect("load")
}

fn plugin() -> Plugin {
    Plugin::new(&bundle(), ID).expect("create")
}

fn latency_normalized(samples: u32) -> f64 {
    f64::from(samples) / f64::from(MAX_LATENCY)
}

#[test]
fn the_bundle_lists_the_plugin() {
    let b = bundle();
    let [d] = b.descriptors() else {
        panic!("one plugin: {:?}", b.descriptors());
    };
    assert_eq!(d.id, ID);
    assert_eq!(d.name, NAME);
    assert_eq!(d.vendor, "Xirius");
    assert!(d.is_effect() && !d.is_instrument());
    assert_eq!(b.find_key(Some(class_key(ID))).map(|d| &d.id), Some(&d.id));
    assert_eq!(b.find_key(None).map(|d| &d.id), Some(&d.id));
    assert!(b.find_key(Some(class_key("other"))).is_none());
    assert!(Plugin::new(&b, "com.xirius.missing").is_err());
}

#[test]
fn parameters_are_read_and_written() {
    let p = plugin();
    let gain = p.param_info(GAIN).expect("gain");
    assert_eq!(gain.title, "Gain");
    assert!(gain.is_automatable() && !gain.is_stepped());
    assert_eq!(gain.default_normalized(), 1.0);
    let lat = p.param_info(LATENCY).expect("latency");
    assert!(lat.is_stepped());
    assert_eq!(lat.step_count(), MAX_LATENCY as i32);

    assert_eq!(p.param(GAIN), Some(1.0));
    assert_eq!(p.param_text(GAIN, 0.25).as_deref(), Some("25.0 %"));
    // Inactive: the value reaches the plugin through `flush`.
    assert!(p.set_param(GAIN, 0.25));
    assert_eq!(p.param(GAIN), Some(0.25));
    assert!(p.set_param(LATENCY, latency_normalized(100)));
    assert_eq!(p.param_plain(LATENCY), Some(100.0));
    assert!(!p.set_param(999, 0.5));
}

#[test]
fn the_state_comes_back_in_a_new_instance() {
    let a = plugin();
    a.set_param(GAIN, 0.75);
    a.set_param(LATENCY, latency_normalized(32));
    let state = a.get_state().expect("save");
    assert_eq!(&state[..4], b"XTCL");
    drop(a);

    let b = plugin();
    assert_eq!(b.param(GAIN), Some(1.0));
    b.set_state(&state).expect("load");
    assert_eq!(b.param(GAIN), Some(0.75));
    assert_eq!(b.param_plain(LATENCY), Some(32.0));
    // The plugin asked for a rescan after the load; that is the host's own change, not an edit.
    assert!(!b.take_params_changed());
    assert!(!b.take_dirty());
    assert_eq!(b.get_state().expect("save again"), state);
    assert!(b.set_state(b"nope").is_err(), "a wrong state is refused");
    assert_eq!(b.param(GAIN), Some(0.75));
}

#[test]
fn edt_134_the_note_names_are_the_plugins_own_for_its_first_port() {
    let p = plugin();
    // The names for another port and for every key are left out; the rest come by key.
    let want: Vec<(u8, String)> = NOTE_NAMES
        .iter()
        .map(|(k, n)| (*k, n.to_string()))
        .collect();
    assert_eq!(p.note_names(), want);
    assert!(!p.take_note_names_changed());

    // A state with a low gain renames the kick, and the plugin says the names changed.
    p.set_param(GAIN, 0.25);
    let state = p.get_state().expect("save");
    p.set_param(GAIN, 1.0);
    p.set_state(&state).expect("load");
    assert!(p.take_note_names_changed());
    assert_eq!(p.note_names()[0], (36, SOFT_KICK.to_string()));
    assert!(!p.take_note_names_changed());
}

#[test]
fn process_applies_gain_notes_and_parameter_changes() {
    let mut p = plugin();
    p.set_param(GAIN, 0.5);
    p.set_param(LATENCY, latency_normalized(7));
    let mut audio = p.audio().expect("audio");
    assert!(p.audio().is_err(), "one audio part at a time");
    assert_eq!(audio.prepare(48_000.0, BLOCK, false).expect("prepare"), 7);
    assert_eq!(audio.input_channels(), vec![2]);
    assert_eq!(audio.output_channels(), vec![2]);
    assert!(audio.has_note_input());

    let (mut l, mut r) = (vec![1.0f32; BLOCK], vec![-1.0f32; BLOCK]);
    audio.process(&mut l, &mut r, &[]);
    assert!(l.iter().all(|&v| v == 0.5), "{l:?}");
    assert!(r.iter().all(|&v| v == -0.5), "{r:?}");

    // A note from frame 16 adds 0.25 × velocity from there on.
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let on = Note {
        offset: 16,
        pitch: 60,
        velocity: 1.0,
        on: true,
    };
    audio.process(&mut l, &mut r, &[on]);
    assert!(l[..16].iter().all(|&v| v == 0.0), "{l:?}");
    assert!(l[16..].iter().all(|&v| v == 0.25), "{l:?}");
    assert_eq!(l, r);

    // The note ends at frame 0; a parameter change at frame 32 changes the gain from there.
    let (mut l, mut r) = (vec![1.0f32; BLOCK], vec![1.0f32; BLOCK]);
    let off = Note {
        offset: 0,
        on: false,
        ..on
    };
    let change = ParamChange {
        offset: 32,
        id: GAIN,
        value: 1.0,
    };
    audio.process_with_params(&mut l, &mut r, &[off], &[], &[change]);
    assert!(l[..32].iter().all(|&v| v == 0.5), "{l:?}");
    assert!(l[32..].iter().all(|&v| v == 1.0), "{l:?}");
    assert_eq!(p.param(GAIN), Some(1.0));

    // While active, an edit from another thread reaches the next block.
    assert!(p.set_param(GAIN, 0.25));
    let (mut l, mut r) = (vec![1.0f32; BLOCK], vec![1.0f32; BLOCK]);
    audio.process(&mut l, &mut r, &[]);
    assert!(l.iter().all(|&v| v == 0.25), "{l:?}");
    assert_eq!(audio.counts(), (0, 0));
}

#[test]
fn an_edit_in_the_plugin_comes_back_as_a_gesture() {
    let mut p = plugin();
    let mut audio = p.audio().expect("audio");
    audio.prepare(44_100.0, BLOCK, false).expect("prepare");
    let (mut l, mut r) = (vec![1.0f32; BLOCK], vec![1.0f32; BLOCK]);
    let key = Note {
        offset: 0,
        pitch: EDIT_KEY as u8,
        velocity: 1.0,
        on: true,
    };
    audio.process(&mut l, &mut r, &[key]);
    assert_eq!(
        p.take_gestures(),
        vec![
            Gesture::Begin(GAIN),
            Gesture::Perform(GAIN, EDIT_GAIN),
            Gesture::End(GAIN),
        ]
    );
    assert!(p.take_gestures().is_empty());
    assert_eq!(p.param(GAIN), Some(EDIT_GAIN));
}

#[test]
fn prepare_again_and_offline_keep_working() {
    let mut p = plugin();
    let mut audio = p.audio().expect("audio");
    audio.prepare(48_000.0, BLOCK, false).expect("prepare");
    let (mut l, mut r) = (vec![1.0f32; BLOCK], vec![1.0f32; BLOCK]);
    audio.process(&mut l, &mut r, &[]);
    // Again for an offline render with a longer block, and with a new latency set while
    // inactive in between.
    assert_eq!(audio.prepare(96_000.0, 2 * BLOCK, true).expect("again"), 0);
    let (mut l, mut r) = (vec![1.0f32; 2 * BLOCK], vec![1.0f32; 2 * BLOCK]);
    audio.process(&mut l, &mut r, &[]);
    assert!(l.iter().all(|&v| v == 1.0));
    drop(audio);
    assert!(p.set_param(LATENCY, latency_normalized(5)));
    let mut audio = p
        .audio()
        .expect("a new audio part after the last one is gone");
    assert_eq!(audio.prepare(48_000.0, BLOCK, false).expect("prepare"), 5);
    assert_eq!(p.latency(), 5);
}

#[test]
fn the_audio_part_runs_on_another_thread_and_outlives_the_handle() {
    let mut p = plugin();
    let mut audio = p.audio().expect("audio");
    audio.prepare(48_000.0, BLOCK, false).expect("prepare");
    let worker = std::thread::spawn(move || {
        let (mut l, mut r) = (vec![1.0f32; BLOCK], vec![1.0f32; BLOCK]);
        for _ in 0..8 {
            audio.process(&mut l, &mut r, &[]);
        }
        (l[0], audio)
    });
    let (v, audio) = worker.join().expect("worker");
    assert_eq!(v, 1.0);
    drop(p);
    // The plugin's main thread stays until the audio part is gone too.
    drop(audio);
}

#[test]
fn an_edit_right_after_the_audio_part_is_dropped_is_not_lost() {
    let mut p = plugin();
    for n in 0..500u32 {
        let mut audio = p.audio().expect("audio");
        audio.prepare(48_000.0, BLOCK, false).expect("prepare");
        drop(audio);
        // The deactivation is still on its way to the main thread here.
        let want = n % 7 + 1;
        assert!(p.set_param(LATENCY, latency_normalized(want)));
        let mut audio = p.audio().expect("audio");
        assert_eq!(
            audio.prepare(48_000.0, BLOCK, false).expect("again"),
            want,
            "round {n}"
        );
    }
}
