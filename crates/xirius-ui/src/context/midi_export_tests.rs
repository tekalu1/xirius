//! UI-281: the track's "書き出す" row has its stem and the MIDI file under it, and a MIDI clip has
//! "MIDI ファイルとして書き出す…", which opens the panel on the MIDI kind over the clips' span.

use super::tests::{choose, enabled, labels, studio};
use super::{Act, Item};
use crate::arrange::probe::{clip_mid, row_mid, track};
use crate::export::Range;
use crate::popup::PopupKind;
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

const STEM: &str = "このトラックのステム…";
const MIDI: &str = "MIDI ファイル…";
const CLIP_MIDI: &str = "MIDI ファイルとして書き出す…";

fn press_track(s: &mut Studio, g: &Geometry, name: &str) -> bool {
    let t = track(s, name);
    let (x, y) = row_mid(s, g, RowKind::Track(t));
    s.scene_context(g, x, y)
}

fn press_clip(s: &mut Studio, g: &Geometry, name: &str, i: usize) -> bool {
    s.popup = None;
    let (x, y) = clip_mid(s, g, name, i);
    s.scene_context(g, x, y)
}

fn export_open(s: &Studio) -> bool {
    s.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Export)
}

#[test]
fn the_export_row_of_a_track_has_its_stem_and_its_midi_file() {
    let (mut s, g) = studio();
    assert!(press_track(&mut s, &g, "Keys"));
    assert!(choose(&mut s, &g, "書き出す"));
    assert_eq!(labels(&s), ["戻る", STEM, MIDI]);
    assert!(enabled(&s, STEM) && enabled(&s, MIDI));
    // An audio track has no notes to write.
    s.popup = None;
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(choose(&mut s, &g, "書き出す"));
    assert!(enabled(&s, STEM));
    assert!(!enabled(&s, MIDI));
}

#[test]
fn the_midi_row_of_a_track_opens_the_panel_on_that_track_over_the_song() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    s.export.range = Range::Loop;
    s.export.each = true;
    let n = s.song.steps();
    assert!(press_track(&mut s, &g, "Keys"));
    assert!(choose(&mut s, &g, "書き出す"));
    assert!(choose(&mut s, &g, MIDI));
    assert!(export_open(&s));
    assert!(s.export.midi && !s.export.stems);
    assert!(!s.export.each, "one file is where it starts");
    assert_eq!(s.export.midi_picked, Some(vec![keys]));
    assert_eq!(s.export.range, Range::Song);
    assert_eq!(s.song.steps(), n);
}

#[test]
fn the_stem_row_of_a_track_still_opens_the_stems() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(choose(&mut s, &g, "書き出す"));
    assert!(choose(&mut s, &g, STEM));
    assert!(export_open(&s));
    assert!(s.export.stems && !s.export.midi);
    assert_eq!(s.export.picked, [vocal]);
}

#[test]
fn a_midi_clip_opens_the_midi_kind_over_the_span_of_the_clips() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let span = {
        let c = &s.project().tracks[&keys].clips[0];
        (c.start, c.end())
    };
    assert!(press_clip(&mut s, &g, "Keys", 0));
    assert!(enabled(&s, CLIP_MIDI));
    assert!(choose(&mut s, &g, CLIP_MIDI));
    assert!(export_open(&s));
    assert!(s.export.midi);
    assert_eq!(s.export.midi_picked, Some(vec![keys]));
    assert_eq!(s.export.range, Range::Selection);
    assert_eq!(s.export.fixed_span, Some(span));
    // An audio clip has no such row.
    assert!(press_clip(&mut s, &g, "Vocal", 0));
    assert!(!labels(&s).iter().any(|l| l == CLIP_MIDI));
}

#[test]
fn the_midi_rows_are_view_export_with_the_midi_kind() {
    let state = |s: &Studio| {
        format!(
            "{:?}",
            (
                s.popup.as_ref().map(|p| p.kind),
                s.export.midi,
                s.export.stems,
                &s.export.midi_picked,
                &s.export.range,
                s.export.fixed_span,
            )
        )
    };
    let (a0, g) = studio();
    let keys = track(&a0, "Keys");
    let bar = super::tests::BAR;
    for (act, line) in [
        (
            Act::MidiExport(vec![keys], None),
            format!("view.export --kind midi --ids {keys}"),
        ),
        (
            Act::MidiExport(vec![keys], Some((bar, 3 * bar))),
            format!("view.export --kind midi --ids {keys} --range 2.1.0..4.1.0"),
        ),
        (
            Act::MidiExport(Vec::new(), None),
            "view.export --kind midi".to_string(),
        ),
    ] {
        let (mut a, _) = studio();
        let (mut b, _) = studio();
        assert!(a.context_run(&g, Item::act("row", "", act)), "{line}");
        assert!(b.run_binding(&g, &line), "{line}");
        assert_eq!(state(&a), state(&b), "{line}");
        assert!(a.export.midi, "{line}");
    }
    let (mut c, _) = studio();
    assert!(c.run_binding(&g, "view.export --kind midi --range loop"));
    assert_eq!(c.export.range, Range::Loop);
    assert_eq!(c.export.midi_picked, None);
    // A kind there is no panel for is refused.
    let (mut d, _) = studio();
    assert!(!d.run_binding(&g, "view.export --kind dawproject"));
    assert!(d.error.is_some() && d.popup.is_none());
}
