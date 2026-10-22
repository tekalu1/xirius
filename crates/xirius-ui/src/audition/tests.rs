//! The audition plays the chosen file on a player of its own, silently in a test (BRW-007).

use super::*;
use crate::browser::RailTab;

/// A place of WAV and MIDI files in a folder of the test's own, which goes when the guard is
/// dropped (also when the test fails).
fn place(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix(&format!("xirius-ui-audition-{name}-"))
        .tempdir()
        .unwrap();
    let top = dir.path().join("素材");
    crate::import_shots::make_place(&top);
    (dir, top)
}

/// How long a wait for the player may take before the test gives up: far over what it needs,
/// for a machine busy with a parallel build.
const GUARD: Duration = Duration::from_secs(60);

fn studio(mode: OutputMode) -> Studio {
    let mut s = Studio::sample();
    s.transport.mode = mode;
    s.browser.press_rail(RailTab::Files);
    s
}

#[test]
fn a_chosen_file_plays_to_its_end_and_lets_go() {
    let (_tmp, top) = place("ends");
    let mut s = studio(OutputMode::Silent { paced: true });
    let kick = top.join("Drums/kick_01.wav").to_string_lossy().into_owned();
    s.browser.add_place(&top).unwrap();
    s.browser.select_file(&kick);
    assert!(s.follow_audition());
    assert!(is_playing());
    let started = Instant::now();
    let mut seen = 0.0f32;
    while s.browser.playing.is_some() {
        assert!(started.elapsed() < GUARD, "still playing at {seen}");
        // Read just before the poll that decides whether it has ended, so the last value read
        // is where it was when it let go.
        if let Some(p) = polled(&kick) {
            seen = seen.max(p);
        }
        s.follow_audition();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(s.error, None);
    assert_eq!(s.browser.playing, None, "it ended by itself");
    assert!(!is_playing());
    assert!(seen > 0.5, "{seen}");
    // Half a second, paced: not over at once.
    assert!(started.elapsed() >= Duration::from_millis(300));
}

#[test]
fn another_file_or_stop_ends_what_plays() {
    let (_tmp, top) = place("switch");
    let mut s = studio(OutputMode::Silent { paced: true });
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    let lp = top
        .join("Loops/loop_120bpm.wav")
        .to_string_lossy()
        .into_owned();
    s.browser.select_file(&pad);
    s.follow_audition();
    assert!(progress(&pad).is_some());
    s.browser.select_file(&lp);
    s.follow_audition();
    assert_eq!(progress(&pad), None);
    assert!(progress(&lp).is_some());
    s.browser.toggle_audition();
    s.follow_audition();
    assert!(!is_playing());
}

#[test]
fn without_sound_the_file_shows_as_playing_and_nothing_starts() {
    let (_tmp, top) = place("none");
    let mut s = studio(OutputMode::None);
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    s.browser.select_file(&pad);
    s.follow_audition();
    assert_eq!(s.browser.playing.as_deref(), Some(pad.as_str()));
    assert!(!is_playing());
    assert_eq!(s.error, None);
}

#[test]
fn a_file_that_cannot_be_read_says_so_and_stops() {
    let (_tmp, top) = place("broken");
    let bad = top.join("broken.wav");
    std::fs::write(&bad, b"RIFF....WAVE").unwrap();
    let mut s = studio(OutputMode::Silent { paced: true });
    s.browser.select_file(&bad.to_string_lossy());
    s.follow_audition();
    assert_eq!(s.browser.playing, None);
    assert!(s.error.is_some());
}

#[test]
fn peaks_go_in_the_songs_folder_for_its_files_and_elsewhere_otherwise() {
    let mut b = Browser::new();
    let song = std::env::temp_dir().join("xirius-ui-audition-song");
    b.song_dir = Some(song.clone());
    assert_eq!(
        store_for(&b, &song.join("Audio/a.wav")),
        peaks::store_dir(Some(&song))
    );
    let other = std::env::temp_dir().join("xirius-ui-audition-peaks");
    b.peak_store = Some(other.clone());
    assert_eq!(store_for(&b, Path::new("D:/elsewhere/b.wav")), other);
}
