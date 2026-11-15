//! MIX-051: the offline builder keeps a guide's notes in a vocal stem.
use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::sync::Arc;

use xirius_core::id::{ClipId, Node, NoteId, PluginId, TrackId};
use xirius_core::model::{
    Clip, ClipBody, Note, NoteSource, Plugin, PluginIsolation, PluginKind, Project, Track,
    TrackKind,
};
use xirius_session::build::Builder;
use xirius_session::export;
use xirius_session::graph::Select;

fn song(file: &std::path::Path) -> (Project, TrackId, PluginId) {
    let mut p = Project::new("guide and voice");
    let guide_id = TrackId(p.alloc());
    let voice_id = TrackId(p.alloc());
    let effect_id = PluginId(p.alloc());
    let mut guide = Track::new(guide_id, TrackKind::Instrument, "melody");
    guide.mute = true;
    let notes = vec![
        Note {
            id: NoteId(p.alloc()),
            start: 0,
            length: 1920,
            pitch: 60,
            velocity: 100,
            muted: false,
            ai: false,
        },
        Note {
            id: NoteId(p.alloc()),
            start: 1920,
            length: 1920,
            pitch: 64,
            velocity: 100,
            muted: false,
            ai: false,
        },
    ];
    guide.clips.push(Clip {
        id: ClipId(p.alloc()),
        name: "guide".into(),
        color: None,
        start: 0,
        length: 3840,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Midi {
            notes: Arc::new(notes),
            transpose: 0,
        },
    });
    let mut voice = Track::new(voice_id, TrackKind::Audio, "voice");
    voice.clips.push(Clip {
        id: ClipId(p.alloc()),
        name: "voice".into(),
        color: None,
        start: 0,
        length: 3840,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: file.to_string_lossy().into_owned(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    });
    voice.inserts.push(Plugin {
        id: effect_id,
        name: "Pitch Correction".into(),
        kind: PluginKind::Effect,
        format: "builtin".into(),
        uid: "pitch-correction".into(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::from([("amount".into(), 1.0)]),
        state: None,
        samples: Vec::new(),
        isolation: PluginIsolation::Dedicated,
        sidechain: None,
        note_source: Some(NoteSource {
            on: true,
            track: guide_id,
        }),
        preset: None,
    });
    p.tracks.insert(guide_id, Arc::new(guide));
    p.tracks.insert(voice_id, Arc::new(voice));
    p.root
        .extend([Node::Track(guide_id), Node::Track(voice_id)]);
    (p, voice_id, effect_id)
}

fn voice_file(dir: &std::path::Path) -> std::path::PathBuf {
    let file = dir.join("voice.wav");
    let mut writer = hound::WavWriter::create(
        &file,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for i in 0..96_000 {
        let hz = if i < 48_000 { 270.0 } else { 340.0 };
        let sample = (0.4 * (TAU * hz * i as f64 / 48_000.0).sin()) as f32;
        writer.write_sample(sample).unwrap();
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
    file
}

fn pitch_at(signal: &[f32], low: f64, high: f64) -> f64 {
    let mut best = (0.0, f64::NEG_INFINITY);
    let mut hz = low;
    while hz <= high {
        let mut re = 0.0;
        let mut im = 0.0;
        for (i, sample) in signal.iter().enumerate() {
            let window = 0.5 - 0.5 * (TAU * i as f64 / signal.len() as f64).cos();
            let phase = TAU * hz * i as f64 / 48_000.0;
            re += f64::from(*sample) * window * phase.cos();
            im += f64::from(*sample) * window * phase.sin();
        }
        let power = re * re + im * im;
        if power > best.1 {
            best = (hz, power);
        }
        hz += 0.5;
    }
    best.0
}

#[test]
fn voice_stem_uses_muted_guide_even_when_guide_audio_is_outside_scope() {
    let dir = tempfile::tempdir().unwrap();
    let file = voice_file(dir.path());
    let (mut p, voice, effect) = song(&file);
    let all = export::render(
        &mut Builder::new(48_000, None),
        &p,
        Select::All,
        0,
        96_000,
        0,
    )
    .unwrap();
    let stem = export::render(
        &mut Builder::new(48_000, None),
        &p,
        Select::Track(voice),
        0,
        96_000,
        0,
    )
    .unwrap();
    assert!(all.warnings.is_empty(), "{:?}", all.warnings);
    assert!(stem.warnings.is_empty(), "{:?}", stem.warnings);
    assert_eq!(stem.left, all.left);
    assert_eq!(stem.right, all.right);
    let guided = pitch_at(&stem.left[25_000..37_000], 245.0, 290.0);
    assert!((guided - 261.63).abs() < 6.0, "guided={guided}");
    Arc::make_mut(p.tracks.get_mut(&voice).unwrap())
        .inserts
        .iter_mut()
        .find(|x| x.id == effect)
        .unwrap()
        .note_source
        .as_mut()
        .unwrap()
        .on = false;
    let off = export::render(
        &mut Builder::new(48_000, None),
        &p,
        Select::Track(voice),
        0,
        96_000,
        0,
    )
    .unwrap();
    let chromatic = pitch_at(&off.left[25_000..37_000], 245.0, 290.0);
    assert!((chromatic - 277.18).abs() < 6.0, "chromatic={chromatic}");
}
