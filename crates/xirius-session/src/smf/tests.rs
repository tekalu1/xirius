//! Reading standard MIDI files built byte by byte.

use super::build::{file, mtrk, three_notes};
use super::*;

fn note(at: i64, length: i64, pitch: u8, velocity: u8) -> SmfNote {
    SmfNote {
        at,
        length,
        pitch,
        velocity,
    }
}

#[test]
fn format_0_notes_are_scaled_to_the_song_ppq() {
    let notes = read(&three_notes(), 960).unwrap();
    assert_eq!(
        notes,
        [
            note(0, 960, 60, 100),
            note(960, 960, 64, 90),
            note(1920, 960, 67, 80)
        ]
    );
}

#[test]
fn format_1_takes_the_notes_of_every_track() {
    let tempo = mtrk(&[(0, &[0xff, 0x51, 0x03, 0x07, 0xa1, 0x20])]);
    let melody = mtrk(&[(0, &[0x90, 72, 100]), (96, &[0x80, 72, 0])]);
    let bass = mtrk(&[(96, &[0x91, 36, 110]), (192, &[0x81, 36, 0])]);
    let notes = read(&file(1, 96, &[tempo, melody, bass]), 960).unwrap();
    assert_eq!(notes, [note(0, 960, 72, 100), note(960, 1920, 36, 110)]);
}

#[test]
fn running_status_and_velocity_0_end_notes() {
    // One status byte for every event; a note on at velocity 0 ends a note.
    let t = mtrk(&[
        (0, &[0x90, 60, 100]),
        (0, &[64, 100]),
        (240, &[60, 0]),
        (240, &[64, 0]),
    ]);
    let notes = read(&file(0, 480, &[t]), 480).unwrap();
    assert_eq!(notes, [note(0, 240, 60, 100), note(0, 480, 64, 100)]);
}

#[test]
fn other_events_are_skipped_and_held_notes_end_with_the_track() {
    let t = mtrk(&[
        (0, &[0xb0, 7, 100]),
        (0, &[0xc0, 5]),
        (0, &[0xf0, 0x03, 0x7e, 0x7f, 0xf7]),
        (0, &[0xff, 0x03, 0x04, b'l', b'e', b'a', b'd']),
        (0, &[0x90, 48, 64]),
        (0, &[0xe0, 0x00, 0x40]),
        (480, &[0xd0, 10]),
    ]);
    let notes = read(&file(0, 480, &[t]), 960).unwrap();
    assert_eq!(notes, [note(0, 960, 48, 64)]);
}

#[test]
fn the_same_pitch_twice_ends_in_the_order_it_started() {
    let t = mtrk(&[
        (0, &[0x90, 60, 100]),
        (100, &[0x90, 60, 50]),
        (100, &[0x80, 60, 0]),
        (100, &[0x80, 60, 0]),
    ]);
    let notes = read(&file(0, 480, &[t]), 480).unwrap();
    assert_eq!(notes, [note(0, 200, 60, 100), note(100, 200, 60, 50)]);
}

#[test]
fn files_it_cannot_read_say_why() {
    assert_eq!(
        read(b"RIFF\0\0\0\0", 960).unwrap_err(),
        "MIDI ファイルではありません"
    );
    let smpte = file(0, 0xe728, &[mtrk(&[])]);
    assert!(read(&smpte, 960).unwrap_err().contains("SMPTE"));
    let format2 = file(2, 480, &[mtrk(&[])]);
    assert!(read(&format2, 960).unwrap_err().contains("形式 2"));
    let whole = three_notes();
    assert_eq!(
        read(&whole[..whole.len() - 5], 960).unwrap_err(),
        "MIDI ファイルが途中で終わっています"
    );
    // Data with no status before it.
    let t = mtrk(&[(0, &[60, 100])]);
    assert!(read(&file(0, 480, &[t]), 960).is_err());
}

#[test]
fn a_format_1_file_keeps_its_tracks_names_tempo_and_meter() {
    let conductor = mtrk(&[
        (0, &[0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]),
        (0, &[0xff, 0x58, 0x04, 3, 2, 24, 8]),
        (288, &[0xff, 0x51, 0x03, 0x09, 0x27, 0xc0]),
    ]);
    let melody = mtrk(&[
        (0, &[0xff, 0x03, 0x06, b'M', b'e', b'l', b'o', b'd', b'y']),
        (0, &[0x90, 72, 100]),
        (96, &[0x80, 72, 0]),
    ]);
    let bass = mtrk(&[(96, &[0x91, 36, 110]), (192, &[0x81, 36, 0])]);
    let song = read_song(&file(1, 96, &[conductor, melody, bass]), 960).unwrap();
    assert_eq!(
        song.parts,
        [
            Part {
                name: Some("Melody".into()),
                notes: vec![note(0, 960, 72, 100)]
            },
            Part {
                name: None,
                notes: vec![note(960, 1920, 36, 110)]
            },
        ],
        "the conductor track has no notes and is no part"
    );
    assert_eq!(song.tempo, [(0, 120.0), (2880, 100.0)]);
    assert_eq!(song.meter, [(0, 3, 4)]);
}

#[test]
fn a_format_0_file_is_split_by_channel() {
    let t = mtrk(&[
        (0, &[0xff, 0x03, 0x04, b'S', b'o', b'n', b'g']),
        (0, &[0x90, 60, 100]),
        (0, &[0x99, 36, 120]),
        (480, &[0x80, 60, 0]),
        (0, &[0x89, 36, 0]),
    ]);
    let song = read_song(&file(0, 480, &[t]), 960).unwrap();
    assert_eq!(
        song.parts,
        [
            Part {
                name: None,
                notes: vec![note(0, 960, 60, 100)]
            },
            Part {
                name: None,
                notes: vec![note(0, 960, 36, 120)]
            },
        ]
    );
    // One channel keeps the track's name.
    let one = read_song(&three_notes(), 960).unwrap();
    assert_eq!(one.parts.len(), 1);
    assert_eq!(one.tempo, [(0, 120.0)]);
    assert!(one.meter.is_empty());
}

#[test]
fn a_name_that_is_not_utf8_is_left_out() {
    // 「ピアノ」 in Shift_JIS.
    let t = mtrk(&[
        (0, &[0xff, 0x03, 0x06, 0x83, 0x73, 0x83, 0x41, 0x83, 0x6d]),
        (0, &[0x90, 60, 100]),
        (480, &[0x80, 60, 0]),
    ]);
    let song = read_song(&file(1, 480, &[t]), 960).unwrap();
    assert_eq!(song.parts[0].name, None);
}
