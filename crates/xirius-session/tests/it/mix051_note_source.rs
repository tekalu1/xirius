//! MIX-051: the offline builder keeps a guide's notes in a vocal stem.
use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::sync::Arc;

use serde_json::json;
use xirius_core::id::{ClipId, FolderId, Node, NoteId, PluginId, TrackId};
use xirius_core::model::{
    Clip, ClipBody, Folder, Frozen, Note, NoteSource, Plugin, PluginIsolation, PluginKind, Project,
    Track, TrackKind,
};
use xirius_core::session::{RunOptions, Session};
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
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
        pad_outputs: Default::default(),
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

#[test]
fn freezing_the_guide_does_not_change_the_vocal_stem() {
    let dir = tempfile::tempdir().unwrap();
    let file = voice_file(dir.path());
    let (mut project, voice, _) = song(&file);
    let source = project
        .tracks
        .values()
        .find(|t| t.kind == TrackKind::Instrument)
        .unwrap()
        .id;
    let render = |p: &Project| {
        export::render(
            &mut Builder::new(48_000, None),
            p,
            Select::Track(voice),
            0,
            96_000,
            0,
        )
        .unwrap()
        .left
    };
    let before = render(&project);
    Arc::make_mut(project.tracks.get_mut(&source).unwrap()).frozen = Some(Frozen {
        file: file.to_string_lossy().into_owned(),
        start: 0,
    });
    assert!(
        before == render(&project),
        "freeze preserves the guide notes"
    );
    Arc::make_mut(project.tracks.get_mut(&source).unwrap()).frozen = None;
    assert!(
        before == render(&project),
        "unfreeze restores the same sound"
    );
}

#[test]
fn freezing_and_bouncing_the_receiving_track_keep_the_guided_pitch() {
    let dir = tempfile::tempdir().unwrap();
    let file = voice_file(dir.path());
    let (project, voice, _) = song(&file);
    let mut session = Session::with_registry(project, shared_registry());
    commands::attach(&mut session, Default::default());
    session.save_as(dir.path().join("song")).unwrap();
    let render = |s: &Session| {
        export::render(
            &mut Builder::new(48_000, s.dir().map(std::path::Path::to_path_buf)),
            s.project(),
            Select::All,
            0,
            96_000,
            0,
        )
        .unwrap()
        .left
    };
    let before = render(&session);
    let guided = pitch_at(&before[25_000..37_000], 245.0, 290.0);
    assert!((guided - 261.63).abs() < 6.0, "before={guided}");
    let args = |id: TrackId| {
        json!({"ids": [id.to_string()]})
            .as_object()
            .unwrap()
            .clone()
    };
    commands::run(
        &mut session,
        None,
        "track.freeze",
        &args(voice),
        &RunOptions::human(),
    )
    .unwrap();
    let frozen = render(&session);
    let frozen_pitch = pitch_at(&frozen[25_000..37_000], 245.0, 290.0);
    assert!((frozen_pitch - 261.63).abs() < 6.0, "frozen={frozen_pitch}");
    commands::run(
        &mut session,
        None,
        "track.unfreeze",
        &args(voice),
        &RunOptions::human(),
    )
    .unwrap();
    commands::run(
        &mut session,
        None,
        "track.bounce",
        &args(voice),
        &RunOptions::human(),
    )
    .unwrap();
    let bounced = render(&session);
    let bounced_pitch = pitch_at(&bounced[25_000..37_000], 245.0, 290.0);
    assert!(
        (bounced_pitch - 261.63).abs() < 6.0,
        "bounced={bounced_pitch}"
    );
}

#[test]
fn guide_inside_a_folder_still_reaches_the_vocal_stem() {
    let dir = tempfile::tempdir().unwrap();
    let file = voice_file(dir.path());
    let (mut project, voice, _) = song(&file);
    let source = project
        .tracks
        .values()
        .find(|t| t.kind == TrackKind::Instrument)
        .unwrap()
        .id;
    let folder = FolderId(project.alloc());
    project.folders.insert(
        folder,
        Folder {
            id: folder,
            name: "instruments".into(),
            color: None,
            bus: None,
            made_bus: None,
            children: vec![Node::Track(source)],
        },
    );
    project.root.retain(|n| *n != Node::Track(source));
    project.root.insert(0, Node::Folder(folder));
    project.validate().unwrap();
    let stem = export::render(
        &mut Builder::new(48_000, None),
        &project,
        Select::Track(voice),
        0,
        96_000,
        0,
    )
    .unwrap();
    let guided = pitch_at(&stem.left[25_000..37_000], 245.0, 290.0);
    assert!((guided - 261.63).abs() < 6.0, "guided={guided}");
}
