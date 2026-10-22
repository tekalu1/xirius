use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::auto::{AutoLane, AutoPoint, AutoTarget};
use crate::id::*;
use crate::marker::Marker;
use crate::model::{
    AudioProcess, AuxSend, Clip, ClipBody, CtrlKind, CtrlLane, CtrlPoint, Folder, Note, Output,
    Plugin, PluginIsolation, PluginKind, Project, TempoPoint, TimeSigPoint, Track, TrackKind,
};

/// The `project.xml` Bitwig Studio 5.0 wrote, from the README of github.com/bitwig/dawproject
/// (MIT, Copyright (c) 2020 Bitwig).
const README: &str = include_str!("testdata/bitwig-5.0-readme.xml");

/// The `n`th colour of the track palette, as a song keeps it.
fn palette(n: usize) -> String {
    use xirius_design::{Theme, TrackColor};
    format!(
        "#{:06x}",
        TrackColor::new(n).content(Theme::Dark).to_rgb_u32()
    )
}

fn add(p: &mut Project, kind: TrackKind, name: &str) -> TrackId {
    let id = TrackId(p.alloc());
    p.tracks.insert(id, Arc::new(Track::new(id, kind, name)));
    p.root.push(Node::Track(id));
    id
}

fn track<'a>(p: &'a Project, name: &str) -> &'a Track {
    p.tracks
        .values()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no track {name}"))
}

fn tm(p: &mut Project, id: TrackId) -> &mut Track {
    Arc::make_mut(p.tracks.get_mut(&id).unwrap())
}

fn builtin(p: &mut Project, uid: &str) -> Plugin {
    let def = xirius_params::find(uid).unwrap();
    Plugin {
        id: PluginId(p.alloc()),
        name: def.name.into(),
        kind: PluginKind::Effect,
        format: "builtin".into(),
        uid: def.uid.into(),
        bypass: false,
        deactivated: false,
        params: def.defaults().map(|(k, v)| (k.to_string(), v)).collect(),
        state: None,
        isolation: PluginIsolation::default(),
    }
}

fn wav_info(path: &Path) -> Option<AudioInfo> {
    path.is_file().then_some(AudioInfo {
        channels: 2,
        sample_rate: 48_000,
        seconds: 10.0,
    })
}

fn opts<'a>(song_dir: &'a Path, audio: &'a dyn Fn(&Path) -> Option<AudioInfo>) -> Export<'a> {
    Export {
        song_dir: Some(song_dir),
        embed_audio: true,
        audio,
    }
}

/// A song with a bit of everything the format carries.
fn song(dir: &Path) -> Project {
    let mut p = Project::new("Round Trip");
    let ppq = i64::from(p.ppq);
    let bar = 4 * ppq;
    let t2 = TempoId(p.alloc());
    p.tempo.push(TempoPoint {
        id: t2,
        tick: 4 * bar,
        bpm: 90.0,
        ramp: false,
    });
    let s2 = TimeSigId(p.alloc());
    p.timesig.push(TimeSigPoint {
        id: s2,
        bar: 9,
        num: 3,
        den: 4,
    });

    let verb = add(&mut p, TrackKind::Bus, "Reverb");
    let keys = add(&mut p, TrackKind::Instrument, "Keys");
    let mut eq = builtin(&mut p, "eq");
    let def = xirius_params::find("eq").unwrap();
    let norm = |k: &str, v: f64| def.param(k).unwrap().1.to_norm(v);
    eq.params.insert("low_cut_on".into(), 1.0);
    eq.params
        .insert("low_cut_freq".into(), norm("low_cut_freq", 120.0));
    eq.params
        .insert("low_cut_slope".into(), norm("low_cut_slope", 2.0));
    eq.params
        .insert("mid1_gain".into(), norm("mid1_gain", -4.5));
    eq.params.insert("mid1_q".into(), norm("mid1_q", 2.0));
    eq.params.insert("high_gain".into(), norm("high_gain", 3.0));
    eq.params.insert("output".into(), norm("output", -1.5));
    let eq_id = eq.id;
    let notes = vec![
        Note {
            id: NoteId(p.alloc()),
            start: 0,
            length: ppq,
            pitch: 60,
            velocity: 100,
            muted: false,
            ai: false,
        },
        Note {
            id: NoteId(p.alloc()),
            start: ppq,
            length: ppq / 2,
            pitch: 64,
            velocity: 37,
            muted: false,
            ai: false,
        },
        Note {
            id: NoteId(p.alloc()),
            start: 3 * ppq,
            length: 2 * ppq,
            pitch: 67,
            velocity: 127,
            muted: false,
            ai: false,
        },
    ];
    let mut bend = CtrlPoint::new(ppq, 4096);
    bend.shape = crate::model::CtrlShape::Linear;
    let clip = Clip {
        id: ClipId(p.alloc()),
        name: "Chords".into(),
        color: Some(palette(3)),
        start: bar,
        length: 2 * bar,
        muted: false,
        take: None,
        controllers: vec![
            CtrlLane {
                kind: CtrlKind::Cc(1),
                points: Arc::new(vec![CtrlPoint::new(0, 0), CtrlPoint::new(2 * ppq, 127)]),
            },
            CtrlLane {
                kind: CtrlKind::PitchBend,
                points: Arc::new(vec![CtrlPoint::new(0, 0), bend]),
            },
        ],
        body: ClipBody::Midi {
            notes: Arc::new(notes),
            transpose: 0,
        },
    };
    let send = SendId(p.alloc());
    {
        let t = tm(&mut p, keys);
        t.color = Some(palette(1));
        t.volume_db = -6.0;
        t.pan = -0.5;
        t.inserts.push(eq);
        t.clips.push(clip);
        t.sends.push(AuxSend {
            id: send,
            to: verb,
            level_db: -10.0,
            pre: true,
            muted_level: None,
        });
        let mut lane = AutoLane::new(AutoTarget::Param(eq_id, "mid1_gain".into()));
        lane.points = Arc::new(vec![
            AutoPoint::new(0, norm("mid1_gain", 0.0)),
            AutoPoint::new(bar, norm("mid1_gain", 6.0)),
        ]);
        t.automation.push(lane);
    }

    let drums = add(&mut p, TrackKind::Audio, "Drums");
    std::fs::write(dir.join("loop.wav"), b"RIFF-loop").unwrap();
    std::fs::write(dir.join("hit.wav"), b"RIFF-hit").unwrap();
    let plain = ClipId(p.alloc());
    let follow = ClipId(p.alloc());
    let fast = ClipId(p.alloc());
    {
        let t = tm(&mut p, drums);
        t.mute = true;
        t.volume_db = 3.0;
        t.clips.push(Clip {
            id: plain,
            name: "Loop".into(),
            color: None,
            start: 2 * ppq,
            length: 4 * ppq,
            muted: false,
            take: None,
            controllers: Vec::new(),
            body: ClipBody::Audio {
                file: "loop.wav".into(),
                offset: ppq,
                gain_db: 0.0,
                process: AudioProcess::default(),
            },
        });
        t.clips.push(Clip {
            id: follow,
            name: "Loop 100".into(),
            color: None,
            start: bar * 2,
            length: bar,
            muted: true,
            take: None,
            controllers: Vec::new(),
            body: ClipBody::Audio {
                file: "loop.wav".into(),
                offset: 0,
                gain_db: 0.0,
                process: AudioProcess {
                    follow: true,
                    tempo: Some(100.0),
                    ..AudioProcess::default()
                },
            },
        });
        t.clips.push(Clip {
            id: fast,
            name: "Hit".into(),
            color: None,
            start: bar * 3,
            length: bar / 2,
            muted: false,
            take: None,
            controllers: Vec::new(),
            body: ClipBody::Audio {
                file: dir.join("hit.wav").to_string_lossy().into_owned(),
                offset: 0,
                gain_db: 0.0,
                process: AudioProcess {
                    speed: 150.0,
                    ..AudioProcess::default()
                },
            },
        });
        let mut lane = AutoLane::new(AutoTarget::Volume);
        let mut a = AutoPoint::new(0, -12.0);
        a.step = true;
        lane.points = Arc::new(vec![a, AutoPoint::new(bar, 0.0)]);
        t.automation.push(lane);
    }
    p.fx_mut(plain).fade_in.length = ppq / 2;
    p.fx_mut(plain).fade_out.length = ppq / 4;

    // A folder whose bus the guitar plays into.
    let gtr = TrackId(p.alloc());
    p.tracks
        .insert(gtr, Arc::new(Track::new(gtr, TrackKind::Audio, "Gtr")));
    let gbus = TrackId(p.alloc());
    p.tracks
        .insert(gbus, Arc::new(Track::new(gbus, TrackKind::Bus, "Band")));
    tm(&mut p, gbus).volume_db = -2.0;
    let fid = FolderId(p.alloc());
    p.folders.insert(
        fid,
        Folder {
            id: fid,
            name: "Band".into(),
            color: Some(palette(5)),
            bus: Some(gbus),
            children: vec![Node::Track(gtr)],
        },
    );
    p.root.push(Node::Folder(fid));
    p.root.push(Node::Track(gbus));

    let mid = MarkerId(p.alloc());
    p.markers.push(Marker {
        id: mid,
        name: "Chorus".into(),
        tick: 4 * bar,
        color: None,
    });
    p.validate().unwrap();
    p
}

#[test]
fn a_song_comes_back_from_its_dawproject() {
    let dir = tempfile::tempdir().unwrap();
    let p = song(dir.path());
    let out = dir.path().join("Round Trip.dawproject");
    let skipped = export(&p, &out, &opts(dir.path(), &wav_info)).unwrap();
    assert!(skipped.is_empty(), "{:?}", skipped.lines());
    super::reference_tests::keep_for_checks(&out);
    assert!(!dir.path().join("Round Trip.dawproject.part").exists());

    let media = dir.path().join("media");
    let got = import(&out, &media).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped.lines());
    let q = &got.project;
    let ppq = i64::from(q.ppq);
    let bar = 4 * ppq;
    assert_eq!(q.name, "Round Trip");

    // Tempo, meter and markers.
    let tempo: Vec<_> = q.tempo.iter().map(|t| (t.tick, t.bpm, t.ramp)).collect();
    assert_eq!(tempo, vec![(0, 120.0, false), (4 * bar, 90.0, false)]);
    let meter: Vec<_> = q.timesig.iter().map(|t| (t.bar, t.num, t.den)).collect();
    assert_eq!(meter, vec![(1, 4, 4), (9, 3, 4)]);
    let marks: Vec<_> = q
        .markers
        .iter()
        .map(|m| (m.tick, m.name.as_str()))
        .collect();
    assert_eq!(marks, vec![(4 * bar, "Chorus")]);

    // The instrument track: fader, pan, colour, send, EQ and its automation, notes, controllers.
    let keys = track(q, "Keys");
    assert_eq!(keys.kind, TrackKind::Instrument);
    assert_eq!(keys.color, Some(palette(1)));
    assert!((keys.volume_db + 6.0).abs() < 1e-6, "{}", keys.volume_db);
    assert!((keys.pan + 0.5).abs() < 1e-6);
    let verb = track(q, "Reverb");
    assert_eq!(verb.kind, TrackKind::Bus);
    assert_eq!(keys.sends.len(), 1);
    assert_eq!(keys.sends[0].to, verb.id);
    assert!(keys.sends[0].pre);
    assert!((keys.sends[0].level_db + 10.0).abs() < 1e-6);
    let eq = &keys.inserts[0];
    assert_eq!((eq.format.as_str(), eq.uid.as_str()), ("builtin", "eq"));
    let src_eq = &track(&p, "Keys").inserts[0];
    for (k, v) in &src_eq.params {
        let back = eq.params[k];
        assert!((back - v).abs() < 1e-6, "{k}: {v} -> {back}");
    }
    let lane = &keys.automation[0];
    assert_eq!(lane.target, AutoTarget::Param(eq.id, "mid1_gain".into()));
    let src_lane = &track(&p, "Keys").automation[0];
    for (a, b) in lane.points.iter().zip(src_lane.points.iter()) {
        assert_eq!(a.tick, b.tick);
        assert!((a.value - b.value).abs() < 1e-6);
    }
    assert_eq!(keys.clips.len(), 1);
    let c = &keys.clips[0];
    assert_eq!((c.start, c.length), (bar, 2 * bar));
    assert_eq!(c.name, "Chords");
    assert_eq!(c.color, Some(palette(3)));
    let notes: Vec<_> = c
        .notes()
        .iter()
        .map(|n| (n.start, n.length, n.pitch, n.velocity))
        .collect();
    assert_eq!(
        notes,
        vec![
            (0, ppq, 60, 100),
            (ppq, ppq / 2, 64, 37),
            (3 * ppq, 2 * ppq, 67, 127)
        ]
    );
    let src = &track(&p, "Keys").clips[0].controllers;
    assert_eq!(c.controllers.len(), 2);
    for (a, b) in c.controllers.iter().zip(src) {
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.points, b.points);
    }

    // The audio track: mute, the three clips, their files, fades and automation.
    let drums = track(q, "Drums");
    assert!(drums.mute);
    assert!((drums.volume_db - 3.0).abs() < 1e-6);
    assert_eq!(drums.clips.len(), 3);
    let ClipBody::Audio {
        file,
        offset,
        process,
        ..
    } = &drums.clips[0].body
    else {
        panic!("not audio")
    };
    assert_eq!(
        (drums.clips[0].start, drums.clips[0].length),
        (2 * ppq, 4 * ppq)
    );
    assert!((offset - ppq).abs() <= 1, "{offset}");
    assert_eq!(*process, AudioProcess::default());
    assert!(Path::new(file).starts_with(&media));
    assert_eq!(std::fs::read(file).unwrap(), b"RIFF-loop");
    let fx = q.fx(drums.clips[0].id).unwrap();
    assert_eq!((fx.fade_in.length, fx.fade_out.length), (ppq / 2, ppq / 4));

    let follow = &drums.clips[1];
    assert!(follow.muted);
    let ClipBody::Audio {
        file: f2, process, ..
    } = &follow.body
    else {
        panic!("not audio")
    };
    assert_eq!(f2, file, "one file inside, one file out");
    assert!(process.follow);
    assert!((process.tempo.unwrap() - 100.0).abs() < 1e-3, "{process:?}");

    let ClipBody::Audio { file, process, .. } = &drums.clips[2].body else {
        panic!("not audio")
    };
    assert!(!process.follow);
    assert!((process.speed - 150.0).abs() < 1e-3, "{process:?}");
    assert_eq!(std::fs::read(file).unwrap(), b"RIFF-hit");
    assert_eq!(got.media.len(), 2);

    let vol = &drums.automation[0];
    assert_eq!(vol.target, AutoTarget::Volume);
    let pts: Vec<_> = vol.points.iter().map(|a| (a.tick, a.step)).collect();
    assert_eq!(pts, vec![(0, true), (bar, false)]);
    assert!((vol.points[0].value + 12.0).abs() < 1e-6);
    assert!(vol.points[1].value.abs() < 1e-6);

    // The folder and the bus it plays into.
    let (fid, folder) = q.folders.iter().next().unwrap();
    assert_eq!(folder.name, "Band");
    assert_eq!(folder.color, Some(palette(5)));
    let gtr = track(q, "Gtr");
    assert_eq!(folder.children, vec![Node::Track(gtr.id)]);
    let bus = &q.tracks[&folder.bus.unwrap()];
    assert_eq!(bus.kind, TrackKind::Bus);
    assert!((bus.volume_db + 2.0).abs() < 1e-6);
    assert_eq!(q.effective_output(gtr.id), Some(Output::Bus(bus.id)));
    let at = q
        .root
        .iter()
        .position(|n| *n == Node::Folder(*fid))
        .unwrap();
    assert_eq!(q.root[at + 1], Node::Track(bus.id));
}

#[test]
fn standalone_master_keeps_routing_settings_plugins_and_automation() {
    let dir = tempfile::tempdir().unwrap();
    let tracks: String = (1..=7)
        .map(|i| {
            format!(
                r#"<Track id="t{i}" name="Audio {i}" contentType="audio">
                    <Channel id="c{i}" role="regular" destination="mc"/>
                </Track>"#
            )
        })
        .collect();
    let xml = format!(
        r#"<Project version="1.0">
          <Application name="Studio Pro" version="8.1.2"/>
          <Structure>
            {tracks}
            <Channel id="mc" role="master" name="Main" audioChannels="2">
              <Devices>
                <ClapPlugin id="verb" deviceID="org.example.verb" deviceName="Verb" deviceRole="audioFX">
                  <State path="plugins/verb.clap-preset"/>
                </ClapPlugin>
              </Devices>
              <Mute value="true"/>
              <Pan unit="normalized" value="0.25"/>
              <Volume id="mv" unit="linear" value="0.5"/>
            </Channel>
          </Structure>
          <Arrangement><Lanes timeUnit="beats"><Lanes track="mc">
            <Points><Target parameter="mv"/>
              <RealPoint time="0" value="0.5"/>
              <RealPoint time="4" value="1"/>
            </Points>
          </Lanes></Lanes></Arrangement>
        </Project>"#
    );
    let mut files = |name: &str| {
        assert_eq!(name, "plugins/verb.clap-preset");
        Some(b"verb state".to_vec())
    };
    let got = import_from(&xml, "Standalone master", &mut files, dir.path()).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped.lines());
    let p = &got.project;
    assert_eq!(p.tracks.len(), 8);
    assert_eq!(p.root.len(), 7, "the master is not an extra audio track");
    for i in 1..=7 {
        assert_eq!(
            p.effective_output(track(p, &format!("Audio {i}")).id),
            Some(Output::Master)
        );
    }
    let master = &p.tracks[&TrackId::MASTER];
    assert!((master.volume_db - 20.0 * 0.5f64.log10()).abs() < 1e-6);
    assert_eq!(master.pan, -0.5);
    assert!(master.mute);
    assert_eq!(master.inserts.len(), 1);
    let verb = &master.inserts[0];
    assert_eq!(verb.uid, "org.example.verb");
    assert_eq!(
        vstpreset::from_text(verb.state.as_deref().unwrap())
            .unwrap()
            .component,
        b"verb state"
    );
    assert_eq!(master.automation.len(), 1);
    let lane = &master.automation[0];
    assert_eq!(lane.target, AutoTarget::Volume);
    assert_eq!(lane.points.len(), 2);
    assert_eq!(lane.points[1].tick, 4 * i64::from(p.ppq));
    assert!(lane.points[1].value.abs() < 1e-6);
}

#[test]
fn standalone_buses_keep_outputs_sends_and_source_order() {
    let dir = tempfile::tempdir().unwrap();
    for role in ["submix", "effect", "regular"] {
        let xml = format!(
            r#"<Project version="1.0">
              <Application name="Other" version="1"/>
              <Structure>
                <Track id="keys" name="Keys" contentType="notes">
                  <Channel id="kc" destination="bc"><Sends>
                    <Send id="send" destination="fxc" type="pre">
                      <Volume unit="decibel" value="-9"/>
                    </Send>
                  </Sends></Channel>
                </Track>
                <Channel id="bc" role="{role}" name="Bus" destination="mc" solo="true">
                  <Volume unit="decibel" value="-3"/>
                </Channel>
                <Channel id="fxc" role="effect" name="Return" destination="bc"/>
                <Channel id="mc" role="master"/>
              </Structure>
            </Project>"#
        );
        let got = import_from(&xml, "Standalone buses", &mut |_| None, dir.path()).unwrap();
        assert!(got.skipped.is_empty(), "{role}: {:?}", got.skipped.lines());
        let p = &got.project;
        let keys = track(p, "Keys");
        let bus = track(p, "Bus");
        let ret = track(p, "Return");
        assert_eq!(
            p.root,
            vec![
                Node::Track(keys.id),
                Node::Track(bus.id),
                Node::Track(ret.id)
            ]
        );
        assert_eq!(bus.kind, TrackKind::Bus);
        assert_eq!(ret.kind, TrackKind::Bus);
        assert_eq!(bus.volume_db, -3.0);
        assert!(bus.solo);
        assert_eq!(p.effective_output(keys.id), Some(Output::Bus(bus.id)));
        assert_eq!(p.effective_output(ret.id), Some(Output::Bus(bus.id)));
        assert_eq!(p.effective_output(bus.id), Some(Output::Master));
        assert_eq!(keys.sends.len(), 1);
        assert_eq!(keys.sends[0].to, ret.id);
        assert_eq!(keys.sends[0].level_db, -9.0);
        assert!(keys.sends[0].pre);
        p.validate().unwrap();
    }
}

#[test]
fn missing_destinations_are_still_reported_with_standalone_channels() {
    let dir = tempfile::tempdir().unwrap();
    let xml = r#"<Project version="1.0">
      <Application name="Other" version="1"/>
      <Structure>
        <Track id="audio" name="Audio" contentType="audio">
          <Channel id="ac" destination="missing"/>
        </Track>
        <Channel id="bc" role="effect" name="Bus" destination="missing"/>
        <Channel id="mc" role="master"/>
      </Structure>
    </Project>"#;
    let got = import_from(xml, "Missing outputs", &mut |_| None, dir.path()).unwrap();
    assert_eq!(got.skipped.lines(), vec!["Audio の出力先", "Bus の出力先"]);
}

#[test]
fn bitwigs_example_reads_as_a_song() {
    let dir = tempfile::tempdir().unwrap();
    let mut asked = Vec::new();
    let mut files = |name: &str| {
        asked.push(name.to_string());
        match name {
            "plugins/d19b1f6e-bbb6-42fe-a6c9-54b41d97a05d.clap-preset" => {
                Some(b"surge state".to_vec())
            }
            "audio/Drumfunk3 170bpm.wav" => Some(b"RIFF-drumfunk".to_vec()),
            _ => None,
        }
    };
    let got = import_from(README, "Bitwig", &mut files, dir.path()).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped.lines());
    let p = &got.project;
    let ppq = i64::from(p.ppq);
    assert_eq!(p.name, "Bitwig");
    assert_eq!(p.tempo.len(), 1);
    assert_eq!(p.tempo[0].bpm, 149.0);
    assert_eq!((p.timesig[0].num, p.timesig[0].den), (4, 4));
    assert_eq!(p.tracks.len(), 3, "Bass, Drumloop and the master");

    let bass = track(p, "Bass");
    assert_eq!(bass.kind, TrackKind::Instrument);
    assert_eq!(bass.color.as_deref(), Some("#a2eabf"));
    assert!((bass.volume_db - 20.0 * 0.659140f64.log10()).abs() < 1e-6);
    assert_eq!(bass.pan, 0.0);
    assert_eq!(p.effective_output(bass.id), Some(Output::Master));
    assert_eq!(bass.inserts.len(), 1);
    let surge = &bass.inserts[0];
    assert_eq!(surge.name, "Surge XT");
    assert_eq!(surge.format, "clap");
    assert_eq!(surge.uid, "org.surge-synth-team.surge-xt");
    assert_eq!(surge.kind, PluginKind::Instrument);
    assert!(!surge.bypass);
    let state = vstpreset::from_text(surge.state.as_deref().unwrap()).unwrap();
    assert_eq!(state.component, b"surge state");
    assert_eq!(bass.clips.len(), 1);
    let c = &bass.clips[0];
    assert_eq!((c.start, c.length), (0, 8 * ppq));
    let notes = c.notes();
    assert_eq!(notes.len(), 9);
    assert!(notes.iter().all(|n| n.velocity == 100));
    let long = notes.iter().find(|n| n.start == 3 * ppq / 2).unwrap();
    assert_eq!((long.pitch, long.length), (53, 5 * ppq / 2));

    let drums = track(p, "Drumloop");
    assert_eq!(drums.kind, TrackKind::Audio);
    assert!((drums.volume_db - 20.0 * 0.177125f64.log10()).abs() < 1e-6);
    assert_eq!(drums.clips.len(), 1);
    let c = &drums.clips[0];
    assert_eq!(c.name, "Drumfunk3 170bpm");
    assert_eq!((c.start, c.length), (0, 8 * ppq));
    let ClipBody::Audio {
        file,
        offset,
        process,
        ..
    } = &c.body
    else {
        panic!("not audio")
    };
    assert_eq!(*offset, 0);
    assert!(process.follow);
    // Eight beats of 149 bpm over 2.82 s of the loop: it was played at 170 bpm.
    assert!((process.tempo.unwrap() - 170.0).abs() < 0.01, "{process:?}");
    assert_eq!(Path::new(file), dir.path().join("Drumfunk3 170bpm.wav"));
    assert_eq!(std::fs::read(file).unwrap(), b"RIFF-drumfunk");
    assert_eq!(got.media, vec![dir.path().join("Drumfunk3 170bpm.wav")]);
    drop(files);
    assert_eq!(asked.len(), 2);
}

#[test]
fn what_a_dawproject_cannot_carry_is_listed_on_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = song(dir.path());
    let keys = track(&p, "Keys").id;
    let pid = PluginId(p.alloc());
    let drums = track(&p, "Drums").id;
    {
        let t = tm(&mut p, keys);
        t.inserts.push(Plugin {
            id: pid,
            name: "Old Synth".into(),
            kind: PluginKind::Effect,
            format: "vst2".into(),
            uid: "old".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            isolation: PluginIsolation::default(),
        });
        let ClipBody::Midi { notes, .. } = &mut t.clips[0].body else {
            unreachable!()
        };
        Arc::make_mut(notes)[1].muted = true;
    }
    {
        let t = tm(&mut p, drums);
        let ClipBody::Audio { process, file, .. } = &mut t.clips[0].body else {
            unreachable!()
        };
        process.reverse = true;
        *file = "gone.wav".into();
        let ClipBody::Audio { gain_db, .. } = &mut t.clips[1].body else {
            unreachable!()
        };
        *gain_db = -3.0;
    }
    let o = opts(dir.path(), &wav_info);
    let listed = skipped_on_export(&p, &o).lines();
    assert_eq!(
        listed,
        vec![
            "Keys の Old Synth",
            "ミュートしたノート 1",
            "見つからないファイル gone.wav",
            "Drums のクリップのゲイン",
        ]
    );
    let out = dir.path().join("Left Out.dawproject");
    assert_eq!(export(&p, &out, &o).unwrap().lines(), listed);
    super::reference_tests::keep_for_checks(&out);
}

#[test]
fn what_a_song_cannot_take_is_listed_on_import() {
    let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Other" version="1"/>
  <Transport><Tempo unit="bpm" value="100" id="t0"/></Transport>
  <Structure>
    <Track contentType="audio" id="a" name="Vox">
      <Channel role="regular" id="ac" destination="mc">
        <Devices>
          <Vst2Plugin deviceID="1234" deviceName="Old Verb" id="d1"/>
          <Vst3Plugin deviceID="ABCD" deviceName="Gone" deviceRole="audioFX" id="d2">
            <State path="plugins/missing.vstpreset"/>
          </Vst3Plugin>
        </Devices>
        <Sends>
          <Send destination="ac" id="s1"><Volume unit="linear" value="1" id="s1v"/></Send>
        </Sends>
      </Channel>
    </Track>
    <Track contentType="audio notes" id="m" name="Master">
      <Channel role="master" id="mc"/>
    </Track>
  </Structure>
  <Arrangement>
    <Lanes timeUnit="beats">
      <Lanes track="a">
        <Clips>
          <Clip time="0" duration="4"><Notes><Note time="0" duration="1" key="60"/></Notes></Clip>
          <Clip time="4" duration="4">
            <Warps contentTimeUnit="seconds" timeUnit="beats">
              <Audio channels="2" sampleRate="48000" duration="2"><File path="audio/none.wav"/></Audio>
              <Warp time="0" contentTime="0"/>
              <Warp time="4" contentTime="2"/>
            </Warps>
          </Clip>
        </Clips>
        <Points><Target parameter="nothing"/><RealPoint time="0" value="1"/></Points>
      </Lanes>
    </Lanes>
  </Arrangement>
  <Scenes><Scene id="sc1"/><Scene id="sc2"/></Scenes>
</Project>"##;
    let dir = tempfile::tempdir().unwrap();
    let got = import_from(xml, "Other", &mut |_| None, dir.path()).unwrap();
    assert_eq!(
        got.skipped.lines(),
        vec![
            "Vox の Old Verb",
            "Vox の Gone の状態",
            "Vox のセンド",
            "Vox のノート",
            "見つからないファイル none.wav",
            "Vox のオートメーション",
            "シーン 2",
        ]
    );
    let vox = track(&got.project, "Vox");
    assert_eq!(vox.inserts.len(), 1, "the plugin without its state stays");
    assert!(vox.inserts[0].state.is_none());
    assert!(vox.sends.is_empty());
    assert!(vox.clips.is_empty());
    assert!(got.media.is_empty());
    assert_eq!(got.project.tempo[0].bpm, 100.0);
}

#[test]
fn a_file_that_is_not_a_dawproject_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.dawproject");
    std::fs::write(&bad, b"not a zip").unwrap();
    assert!(import(&bad, dir.path()).is_err());
    assert!(import_from("<Song/>", "x", &mut |_| None, dir.path()).is_err());
    assert!(import_from("<Project", "x", &mut |_| None, dir.path()).is_err());
    assert!(is_dawproject(Path::new("a/Song.DAWproject")));
    assert!(!is_dawproject(Path::new("a/song.xirius")));
}

#[test]
fn opening_a_dawproject_makes_a_new_song_not_saved_yet() {
    use crate::session::{RunOptions, Session};
    let dir = tempfile::tempdir().unwrap();
    let p = song(dir.path());
    let out = dir.path().join("Demo Song.dawproject");
    export(&p, &out, &opts(dir.path(), &wav_info)).unwrap();

    let mut s = Session::new("Before");
    let media = dir.path().join("taken");
    let path = out.display().to_string();
    let dry = s
        .run_value(
            "project.open",
            serde_json::json!({ "path": path, "media": media.display().to_string() }),
            &RunOptions::human().dry(),
        )
        .unwrap();
    assert!(
        dry.message.contains("新しい曲として開きます"),
        "{}",
        dry.message
    );
    assert_eq!(s.project().name, "Before");

    let r = s
        .run_value(
            "project.open",
            serde_json::json!({ "path": path, "media": media.display().to_string() }),
            &RunOptions::human(),
        )
        .unwrap();
    // Named after the file, with no folder until it is saved.
    assert_eq!(s.project().name, "Demo Song");
    assert!(s.dir().is_none());
    assert_eq!(r.data["name"], "Demo Song");
    assert_eq!(r.data["skipped"], serde_json::json!([]));
    assert_eq!(
        track(s.project(), "Drums").clips.len(),
        track(&p, "Drums").clips.len()
    );
    // The audio was taken out to the folder named, and the clips name it there.
    let taken: Vec<_> = std::fs::read_dir(&media).unwrap().collect();
    assert!(!taken.is_empty());
    let named: Vec<String> = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.clips)
        .filter_map(|c| match &c.body {
            ClipBody::Audio { file, .. } => Some(file.clone()),
            _ => None,
        })
        .collect();
    assert!(!named.is_empty());
    assert!(
        named.iter().all(|f| Path::new(f).starts_with(&media)),
        "{named:?}"
    );
}

#[test]
fn a_skipped_row_names_the_thing_and_how_many_apart() {
    let mut s = Skips::default();
    s.item("Keys の Old Synth");
    s.count("ミュートしたノート", 4, "");
    s.count("Kick のワープ", 2, "か所");
    s.item("シーン 2");
    s.item("シーン 2");
    let rows: Vec<_> = s
        .rows()
        .iter()
        .map(|r| (r.label.as_str(), r.detail()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("Keys の Old Synth", String::new()),
            ("ミュートしたノート", "4".to_string()),
            ("Kick のワープ", "2 か所".to_string()),
            ("シーン 2", "2".to_string()),
        ]
    );
}

#[test]
fn reading_says_how_far_it_is_up_to_the_whole() {
    let dir = tempfile::tempdir().unwrap();
    let p = song(dir.path());
    let out = dir.path().join("Steps.dawproject");
    export(&p, &out, &opts(dir.path(), &wav_info)).unwrap();
    let mut seen = Vec::new();
    let got = import_with(&out, &dir.path().join("media"), &mut |f| seen.push(f)).unwrap();
    assert!(!got.media.is_empty());
    assert!(seen.len() > 2, "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    assert!(seen[0] > 0.0 && seen[0] < 1.0);
    assert_eq!(seen.last(), Some(&1.0));
}

/// A VST3 or CLAP plugin of the song.
fn plugin(p: &mut Project, format: &str, uid: &str, name: &str, kind: PluginKind) -> Plugin {
    let state = vstpreset::State {
        component: format!("{name} component").into_bytes(),
        controller: if format == "vst3" {
            format!("{name} controller").into_bytes()
        } else {
            Vec::new()
        },
    };
    Plugin {
        id: PluginId(p.alloc()),
        name: name.into(),
        kind,
        format: format.into(),
        uid: uid.into(),
        bypass: false,
        deactivated: false,
        params: [("3".to_string(), 0.25), ("17".to_string(), 1.0)].into(),
        state: Some(vstpreset::to_text(&state)),
        isolation: PluginIsolation::default(),
    }
}

/// A song with a VST3 instrument and a CLAP effect on one track.
fn song_with_plugins() -> Project {
    let mut p = Project::new("Plugins");
    let pad = add(&mut p, TrackKind::Instrument, "Pad");
    let synth = plugin(
        &mut p,
        "vst3",
        "565354414D504C4573616D706C650000",
        "Diva",
        PluginKind::Instrument,
    );
    let mut verb = plugin(
        &mut p,
        "clap",
        "com.example.verb",
        "Verb",
        PluginKind::Effect,
    );
    verb.bypass = true;
    tm(&mut p, pad).inserts = vec![synth, verb];
    p
}

#[test]
fn a_plugins_state_goes_with_it_both_ways() {
    let dir = tempfile::tempdir().unwrap();
    let p = song_with_plugins();
    let out = dir.path().join("Plugins.dawproject");
    let left = export(&p, &out, &opts(dir.path(), &wav_info)).unwrap();
    assert!(left.is_empty(), "{:?}", left.lines());
    super::reference_tests::keep_for_checks(&out);

    // The VST3 state is a `.vstpreset` that names the plugin by its class id.
    let f = std::fs::File::open(&out).unwrap();
    let mut zip = zip::ZipReader::new(std::io::BufReader::new(f)).unwrap();
    let preset = zip
        .names()
        .iter()
        .find(|n| n.ends_with(".vstpreset"))
        .cloned()
        .unwrap();
    let (class, state) = vstpreset::decode(&zip.read(&preset).unwrap().unwrap()).unwrap();
    assert_eq!(class, "565354414D504C4573616D706C650000");
    assert_eq!(state.component, b"Diva component");
    assert_eq!(state.controller, b"Diva controller");

    let got = import(&out, &dir.path().join("media")).unwrap();
    assert!(got.skipped.is_empty(), "{:?}", got.skipped.lines());
    let was = &track(&p, "Pad").inserts;
    let now = &track(&got.project, "Pad").inserts;
    assert_eq!(now.len(), 2);
    for (a, b) in was.iter().zip(now.iter()) {
        assert_eq!(
            (&a.name, &a.format, &a.uid, a.kind, a.bypass),
            (&b.name, &b.format, &b.uid, b.kind, b.bypass)
        );
        assert_eq!(a.params, b.params, "{}", a.name);
        assert_eq!(a.state, b.state, "{}", a.name);
    }
}

#[test]
fn opening_a_dawproject_lists_the_plugins_this_pc_does_not_have() {
    use crate::command::{Args, CommandDef, Output};
    use crate::error::CmdResult;
    use crate::extension::{ExtContext, Extension};
    use crate::session::{RunOptions, Session};

    /// A PC that has the VST3 and not the CLAP.
    struct Scan;
    impl Extension for Scan {
        fn answers(&self, _: &str) -> bool {
            false
        }
        fn run(
            &mut self,
            _: &ExtContext<'_>,
            _: &CommandDef,
            _: &Args,
            _: bool,
        ) -> CmdResult<Output> {
            unreachable!("it answers nothing")
        }
        fn has_plugin(&mut self, x: &Plugin) -> Option<bool> {
            Some(x.format == "vst3")
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("Plugins.dawproject");
    export(&song_with_plugins(), &out, &opts(dir.path(), &wav_info)).unwrap();
    let mut s = Session::new("Before");
    s.add_extension(Box::new(Scan));
    let media = dir.path().join("taken");
    let r = s
        .run_value(
            "project.open",
            serde_json::json!({ "path": out.display().to_string(), "media": media.display().to_string() }),
            &RunOptions::human(),
        )
        .unwrap();
    assert_eq!(
        r.data["skipped"],
        serde_json::json!(["Pad の Verb（この PC に無い）"])
    );
    assert!(
        r.message.contains("読み込めなかったもの 1"),
        "{}",
        r.message
    );
    // Both stay in the song, the missing one with its state for a PC that has it.
    let pad = track(s.project(), "Pad");
    assert_eq!(pad.inserts.len(), 2);
    assert!(pad.inserts[1].state.is_some());
}

#[test]
fn a_tracks_transpose_and_velocity_go_out_in_the_notes_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = song(dir.path());
    let keys = track(&p, "Keys").id;
    {
        let t = tm(&mut p, keys);
        assert!(t.clips[0].set_transpose(2));
        t.transpose = 3;
        t.velocity = 10;
    }
    let out = dir.path().join("TrackValues.dawproject");
    let skipped = export(&p, &out, &opts(dir.path(), &wav_info)).unwrap();
    assert!(skipped.is_empty(), "{:?}", skipped.lines());
    let got = import(&out, &dir.path().join("media")).unwrap();
    let t = track(&got.project, "Keys");
    // A DAWproject has nowhere to keep either value: the file holds the notes as they play,
    // and the track comes back at 0 (UI-312).
    assert_eq!((t.transpose, t.velocity), (0, 0));
    let clip = &t.clips[0];
    let mut notes: Vec<(u8, u8)> = clip.notes().iter().map(|n| (n.pitch, n.velocity)).collect();
    notes.sort();
    assert_eq!(notes, [(65, 110), (69, 41), (72, 127)]);
}

#[test]
fn a_midi_transpose_goes_out_in_the_notes_and_comes_back_as_zero() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = song(dir.path());
    let keys = track(&p, "Keys").id;
    {
        let t = tm(&mut p, keys);
        assert!(t.clips[0].set_transpose(2));
    }
    let out = dir.path().join("Transpose.dawproject");
    let skipped = export(&p, &out, &opts(dir.path(), &wav_info)).unwrap();
    assert!(skipped.is_empty(), "{:?}", skipped.lines());
    let got = import(&out, &dir.path().join("media")).unwrap();
    let clip = &track(&got.project, "Keys").clips[0];
    // A DAWproject has nowhere to keep the value: the file holds the notes as they play, and
    // the clip comes back at 0 (UI-311).
    assert_eq!(clip.transpose(), 0);
    let pitches: Vec<u8> = clip.notes().iter().map(|n| n.pitch).collect();
    assert_eq!(pitches, [62, 66, 69]);
}
