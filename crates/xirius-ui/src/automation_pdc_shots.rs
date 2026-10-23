use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
    tree::RowKind,
};
use std::{collections::BTreeMap, sync::Arc};
use xirius_core::{
    auto::{AutoLane, AutoPoint, AutoTarget},
    id::{PluginId, TrackId},
    model::{ClipBody, Plugin, PluginIsolation, PluginKind},
};
const START: i64 = 15360;
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "pdc01-lane",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(prepare), 0),
            Act::Press(Target::Track("Vocal")),
            Act::Key("a"),
        ],
    },
    Shot {
        name: "pdc02-playing",
        acts: &[
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Studio(heard), 0),
        ],
    },
    Shot {
        name: "pdc03-stopped",
        acts: &[Act::Press(Target::Top(BarButton::Stop))],
    },
];
fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}
fn prepare(s: &mut Studio, _: u8) {
    let base = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let direct = base.join("xirius_test_vst3.dll");
    let dll = if direct.is_file() {
        direct
    } else {
        base.join("deps/xirius_test_vst3.dll")
    };
    assert!(dll.is_file(), "build xirius-test-vst3 first");
    let entries = xirius_plugins::probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let catalog = Arc::new(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("automation-pdc-ui-{}-{next}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("pdc.wav");
    xirius_session::export::write_wav(&file, &vec![0.125; 192000], &vec![0.125; 192000], 48000, 32)
        .unwrap();
    let mut project = (*s.project()).clone();
    let id = PluginId(project.alloc());
    for t in project.tracks.values_mut() {
        let t = Arc::make_mut(t);
        t.mute = !matches!(t.name.as_str(), "Vocal" | "Guitar");
        if !t.mute {
            t.volume_db = 0.0;
            t.pan = 0.0;
            t.inserts.clear();
            let mut clip = t.clips[0].clone();
            clip.start = START;
            clip.length = 7680;
            clip.body = ClipBody::Audio {
                file: file.to_string_lossy().into_owned(),
                offset: 0,
                gain_db: 0.0,
                process: Default::default(),
            };
            t.clips = vec![clip];
            let mut lane = AutoLane::new(AutoTarget::Mute);
            lane.points = Arc::new(vec![
                AutoPoint::new(START, 1.0),
                AutoPoint::new(START + 3840, 0.0),
            ]);
            t.automation = vec![lane];
            if t.name == "Vocal" {
                t.inserts.push(Plugin {
                    id,
                    name: "Test Latency".into(),
                    kind: PluginKind::Effect,
                    format: "vst3".into(),
                    uid: uid.clone(),
                    bypass: false,
                    deactivated: false,
                    params: BTreeMap::from([("1".into(), 1.0)]),
                    state: None,
                    isolation: PluginIsolation::InProcess,
                    sidechain: None,
                });
            }
        }
    }
    project.validate_deep().unwrap();
    let mut song =
        xirius_core::Session::with_registry(project, xirius_session::commands::shared_registry());
    let song_dir = dir.join("song");
    song.save_as(&song_dir).unwrap();
    assert!(s.command(|song| song.open(&song_dir)));
    s.drawer_open = false;
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    s.transport.set_catalog(catalog);
    s.beat_w = 128.0;
    s.scroll.0 = s.ticks_to_px(START);
    s.scroll.1 = s
        .tree()
        .row_of(RowKind::Track(track(s, "Vocal")))
        .unwrap()
        .y;
    s.seek(START);
}
fn heard(s: &mut Studio, _: u8) {
    let vocal = track(s, "Vocal");
    let peak = std::cell::Cell::new(0.0f32);
    crate::shot::run_for(s, 8.0, |s| {
        peak.set(
            peak.get().max(
                s.mixer
                    .meters
                    .get(xirius_session::host::Strip::Track(vocal))
                    .hold[0],
            ),
        );
        s.playhead >= START + 4800
    });
    assert!(s.playhead >= START + 4800, "{:?}", s.error);
    assert!(
        peak.get() > crate::mixer::meters::level_t(0.1),
        "silent output held meter {}",
        peak.get()
    );
    let host = s.transport.host().unwrap();
    let shape = host
        .plugins()
        .local(s.project().tracks[&vocal].inserts[0].id)
        .unwrap();
    assert_eq!(shape.0.latency(), 256);
    assert_eq!(
        s.project().tracks[&vocal].automation[0].value_at(s.playhead as f64),
        Some(0.0)
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Hit;
    #[test]
    fn the_saved_pdc_song_opens_its_lane_through_the_track_and_key() {
        xirius_test_vst3::fixture_marker();
        let mut s = Studio::sample();
        prepare(&mut s, 0);
        let t = track(&s, "Vocal");
        let g = s.geometry(1600.0, 900.0);
        assert!(s.press(&g, Hit::Row(RowKind::Track(t))));
        assert!(s.mapped_key(&g, "a", None, crate::tools::Mods::default()));
        assert!(s.arrange.open.contains(&t));
        assert_eq!(
            s.project().tracks[&t].automation[0].target,
            AutoTarget::Mute
        );
        let mut builder = xirius_session::build::Builder::new(48000, s.song.dir());
        let deps = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let entries = xirius_plugins::probe(&deps.join("xirius_test_vst3.dll")).unwrap();
        builder.set_catalog(Some(Arc::new(xirius_plugins::Catalog {
            entries,
            failures: Vec::new(),
        })));
        let built = builder.build(&s.project(), xirius_session::graph::Select::All);
        let graph = built.graph;
        let plan = xirius_engine::Plan::build(
            built.spec,
            xirius_session::build::setup(48000, xirius_engine::RenderMode::Offline),
            None,
        )
        .unwrap();
        assert_eq!(plan.latency(), 256);
        let timing = xirius_session::automation_timing::Timing::new(
            &s.project(),
            &graph,
            &plan.shape(),
            builder.plugins(),
        )
        .with_start(384000);
        let map = xirius_session::tempo::TempoMap::from_project(&s.project());
        assert_eq!(
            timing.tick(t, &AutoTarget::Mute, 384960, &map, 48000.0),
            15388.0
        );
        assert_eq!(
            timing.tick(
                track(&s, "Guitar"),
                &AutoTarget::Mute,
                384960,
                &map,
                48000.0
            ),
            15398.0
        );
    }
}
