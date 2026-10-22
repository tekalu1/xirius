//! UI-280: the panel's third kind writes the notes of the instrument tracks as MIDI files into the
//! song's `MIDI` folder, as one file or one a track, and its card lists them like the stems'.

use super::*;
use crate::studio::Geometry;

fn act(s: &mut Studio, g: &Geometry, a: Act) {
    assert!(s.popup_action(g, Action::Xp(a)));
}

fn view(s: &Studio) -> View {
    *s.popup
        .as_ref()
        .and_then(|p| p.export.clone())
        .expect("the panel is open")
}

fn texts(s: &Studio, g: &Geometry) -> Vec<String> {
    s.popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .into_iter()
        .map(|c| c.text)
        .collect()
}

fn looks(s: &Studio, g: &Geometry) -> Vec<Look> {
    s.popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .into_iter()
        .map(|c| c.look)
        .collect()
}

fn live(s: &Studio, g: &Geometry) -> Vec<Act> {
    s.popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .into_iter()
        .filter(|c| c.live())
        .filter_map(|c| match c.action {
            Some(Action::Xp(a)) => Some(a),
            _ => None,
        })
        .collect()
}

/// The sample saved in a folder of its own, which goes when the guard is dropped.
fn saved(tag: &str) -> (Studio, Geometry, std::path::PathBuf, tempfile::TempDir) {
    let tmp = tempfile::Builder::new()
        .prefix(&format!("xirius-midi-ui-{tag}-"))
        .tempdir()
        .unwrap();
    let dir = tmp.path().join("song");
    let mut s = Studio::sample();
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    let g = s.geometry(1600.0, 900.0);
    s.open_export(Rect::new(1500.0, -40.0, 28.0, 28.0));
    act(&mut s, &g, Act::Midi);
    (s, g, dir, tmp)
}

/// The files a card's export wrote, taken away first so the delete sends nothing to the bin.
fn delete(s: &mut Studio, g: &Geometry, job: &str) {
    let j = s.export_job(job).cloned().expect("the record");
    for f in j["result"]["files"].as_array().unwrap() {
        std::fs::remove_file(f["path"].as_str().unwrap()).unwrap();
    }
    act(s, g, Act::Delete(job.to_string()));
    assert!(s.error.is_none(), "{:?}", s.error);
}

#[test]
fn the_midi_kind_lists_the_instrument_tracks_and_leaves_the_format_out() {
    let (mut s, g, _dir, _tmp) = saved("kind");
    let v = view(&s);
    assert!(v.midi && !v.stems);
    let ins = midi_export::instrument_tracks(&s.project());
    assert!(ins.len() > 1, "the sample has instrument tracks");
    assert_eq!(v.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), ins);
    assert!(v.tracks.iter().all(|t| t.on));
    assert!(v.buses.is_empty());
    assert_eq!(v.count, 1);
    assert_eq!(v.name, midi_export::ONE_NAME);
    assert!(v.can_go());
    let t = texts(&s, &g);
    for word in ["MIDI", "1 つにまとめる", "トラックごと"] {
        assert!(t.iter().any(|x| x == word), "{word}: {t:?}");
    }
    assert!(!t.iter().any(|x| x == "WAV" || x == "形式"), "{t:?}");
    assert!(
        looks(&s, &g)
            .iter()
            .any(|l| matches!(l, Look::Xp(XpLook::Field { ext: ".mid", .. })))
    );

    // One a track counts the ticked tracks; none ticked holds the button back.
    act(&mut s, &g, Act::Each(true));
    let v = view(&s);
    assert_eq!(v.count, ins.len());
    assert_eq!(v.name, midi_export::EACH_NAME);
    act(&mut s, &g, Act::Pick(ins[0]));
    assert_eq!(view(&s).count, ins.len() - 1);
    for id in &ins[1..] {
        act(&mut s, &g, Act::Pick(*id));
    }
    assert!(!view(&s).can_go());
    assert!(!live(&s, &g).contains(&Act::Go));
    // Back to the mixdown: the stems' and the mix's settings are as they were.
    act(&mut s, &g, Act::Mode(false));
    let v = view(&s);
    assert!(!v.midi && !v.stems && v.can_go());
}

#[test]
fn a_file_for_each_track_needs_the_track_in_its_name() {
    let (mut s, g, _dir, _tmp) = saved("name");
    act(&mut s, &g, Act::Each(true));
    act(&mut s, &g, Act::EditName);
    s.popup.as_mut().unwrap().naming = Some("notes".into());
    assert!(s.typed("enter", None));
    let v = view(&s);
    assert_eq!(v.name, "notes");
    assert!(v.bad_name() && !v.can_go());
    assert!(
        texts(&s, &g)
            .iter()
            .any(|t| t.contains(TRACK_TAG) && t.contains("必要"))
    );
    // One file takes any name; each kind keeps its own.
    act(&mut s, &g, Act::Each(false));
    let v = view(&s);
    assert_eq!(v.name, midi_export::ONE_NAME);
    assert!(!v.bad_name() && v.can_go());
}

#[test]
fn one_file_is_written_into_the_midi_folder_and_its_card_cannot_be_listened_to() {
    let (mut s, g, dir, _tmp) = saved("one");
    act(&mut s, &g, Act::Go);
    assert!(s.error.is_none(), "{:?}", s.error);
    let v = view(&s);
    assert_eq!(v.cards.len(), 1);
    let card = v.cards[0].clone();
    assert_eq!(card.state, State::Done, "{}", card.error);
    assert!(card.midi);
    assert!(card.title.ends_with(".mid"), "{}", card.title);
    assert!(
        card.meta.starts_with("MIDI · 1 つにまとめる · "),
        "{}",
        card.meta
    );
    assert!(card.kids.is_empty() && card.loudness.is_empty());
    assert_eq!(card.stale, Some(false));
    let file = dir.join(midi_export::FOLDER).join(&card.title);
    assert!(file.is_file(), "{}", file.display());
    let acts = live(&s, &g);
    assert!(!acts.contains(&Act::Preview(card.job.clone())));
    assert!(acts.contains(&Act::Import(card.job.clone())));
    assert!(texts(&s, &g).iter().any(|t| t == "曲に取り込む"));

    // A change to the notes' track puts the card out of date.
    let keys = crate::arrange::probe::track(&s, "Keys");
    s.command(|song| {
        song.run(
            "track.rename",
            json!({ "id": keys.to_string(), "name": "Piano" }),
        )
        .map(|_| ())
    });
    s.refresh_popup();
    assert_eq!(view(&s).cards[0].stale, Some(true));
    assert!(live(&s, &g).contains(&Act::Update(card.job.clone())));
    delete(&mut s, &g, &card.job);
    assert!(view(&s).cards.is_empty());
}

#[test]
fn a_file_for_each_track_lists_the_files_under_one_card() {
    let (mut s, g, dir, _tmp) = saved("each");
    act(&mut s, &g, Act::Each(true));
    let ins = midi_export::instrument_tracks(&s.project());
    act(&mut s, &g, Act::Go);
    assert!(s.error.is_none(), "{:?}", s.error);
    let card = view(&s).cards[0].clone();
    assert_eq!(card.state, State::Done, "{}", card.error);
    assert_eq!(card.title, format!("MIDI {} 本", ins.len()));
    assert!(
        card.meta.starts_with("MIDI · トラックごと · "),
        "{}",
        card.meta
    );
    assert_eq!(card.kids.len(), ins.len());
    for k in &card.kids {
        assert_eq!(k.tracks, Some(1), "{}", k.file);
        assert!(k.color.is_some() && k.file.ends_with(".mid"));
        assert!(dir.join(midi_export::FOLDER).join(&k.file).is_file());
    }
    let rows: Vec<Option<usize>> = looks(&s, &g)
        .into_iter()
        .filter_map(|l| match l {
            Look::Xp(XpLook::File(k)) => Some(k.tracks),
            _ => None,
        })
        .collect();
    assert_eq!(rows, vec![Some(1); ins.len()]);

    // The same settings again: the kind, one a track, and the tracks it wrote.
    let job = card.job.clone();
    act(&mut s, &g, Act::Mode(true));
    assert!(s.export_like(&job));
    assert!(s.export.midi && s.export.each && !s.export.stems);
    assert_eq!(s.export.midi_picked.as_deref(), Some(ins.as_slice()));
    assert_eq!(s.export.name(), midi_export::EACH_NAME);
    delete(&mut s, &g, &job);
}

#[test]
fn the_span_of_a_selection_is_written_and_kept_for_the_same_settings() {
    let (mut s, g, _dir, _tmp) = saved("span");
    let bar = 3840;
    s.export.range = Range::Selection;
    s.export.fixed_span = Some((bar, 3 * bar));
    s.export.fixed_song = s.song.dir();
    s.refresh_popup();
    act(&mut s, &g, Act::Go);
    assert!(s.error.is_none(), "{:?}", s.error);
    let job = view(&s).cards[0].job.clone();
    let j = s.export_job(&job).cloned().unwrap();
    assert_eq!(j["args"]["range"], "custom");
    assert_eq!(
        (j["result"]["from"].as_i64(), j["result"]["to"].as_i64()),
        (Some(bar), Some(3 * bar))
    );
    s.export.range = Range::Song;
    s.export.fixed_span = None;
    assert!(s.export_like(&job));
    assert_eq!(s.export.range, Range::Selection);
    assert_eq!(s.export.fixed_span, Some((bar, 3 * bar)));
    delete(&mut s, &g, &job);
}
