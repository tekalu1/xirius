//! Coarse ARA control messages for the plugin host process (PLG-078).
//! Source and rendered samples travel in immutable named mappings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::Arc;

use crate::document::{FactoryInfo, Note, RegionProps};
use crate::host::{Request, SourceAudio};
use crate::service::{self, Phase, Probe, Rendered, Song};
use crate::shared_audio::SharedAudio;

const MAX_MESSAGE: usize = 64 * 1024 * 1024;
/// 4: the editor's title bar and its presses (UI-380).
pub const PROTOCOL: u32 = 4;

#[derive(Serialize, Deserialize)]
pub enum Command {
    Open {
        protocol: u32,
        format: String,
        path: String,
        id: String,
        name: String,
        archive: Option<(Vec<u8>, String)>,
        fixture: Option<crate::fake::Config>,
    },
    Sync(WireSong),
    Poll,
    Store {
        changed: bool,
    },
    Notes {
        key: u64,
        modification: bool,
    },
    Probe,
    Edit {
        title: String,
        place: WirePlacement,
        clips: Vec<u64>,
    },
    Caption(WireCaption),
    Bar(WireBar),
    CloseEditor,
    MarkChanged,
    FixtureEdit {
        key: u64,
        gain: f32,
    },
    Close,
}

#[derive(Serialize, Deserialize)]
pub enum Reply {
    Ack,
    Error(String),
    Snapshot(Snapshot),
    Archive {
        edited: Option<(Vec<u8>, String)>,
        analysis: Option<(Vec<u8>, String)>,
    },
    Notes(Vec<Note>),
    Probe(Option<Probe>),
}

#[derive(Serialize, Deserialize)]
pub struct WireSong {
    pub tempos: Vec<(f64, f64)>,
    pub bars: Vec<(f64, i32, i32)>,
    pub bpm: f64,
    pub numerator: i32,
    pub denominator: i32,
    pub clips: Vec<WireClip>,
}

#[derive(Serialize, Deserialize)]
pub struct WireClip {
    pub regions: Vec<RegionProps>,
    pub key: u64,
    pub clone_of: Option<u64>,
    pub source: String,
    pub name: String,
    pub mapping: String,
    pub start: f64,
    pub offset: f64,
    pub duration: f64,
}

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub states: Vec<(u64, WireState)>,
    pub crashed: Option<String>,
    pub failed: Option<String>,
    pub loaded: bool,
    pub dirty: bool,
    pub editing: bool,
    pub editor_handle: u64,
    pub editor_error: Option<String>,
    pub analysis_archive: Option<(Vec<u8>, String)>,
    pub requests: Vec<Request>,
    pub info: Option<FactoryInfo>,
    pub generation: u64,
    /// What was pressed on the editor's title bar since the last poll (UI-380).
    #[serde(default)]
    pub editor_actions: Vec<WireAction>,
}

#[derive(Serialize, Deserialize)]
pub struct WireState {
    pub phase: Phase,
    pub progress: f32,
    pub rendered: Option<WireRender>,
}

#[derive(Serialize, Deserialize)]
pub struct WireRender {
    pub name: String,
    pub rate: f64,
    pub duration: f64,
    pub offset: f64,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct WireCaption {
    pub dark: bool,
    pub fill: u32,
    pub text: u32,
}

impl From<service::Caption> for WireCaption {
    fn from(c: service::Caption) -> Self {
        Self {
            dark: c.dark,
            fill: c.fill,
            text: c.text,
        }
    }
}

impl From<WireCaption> for service::Caption {
    fn from(c: WireCaption) -> Self {
        Self {
            dark: c.dark,
            fill: c.fill,
            text: c.text,
        }
    }
}

/// The ARA editor's title bar (UI-380): only the name and the pin are drawn, and the window
/// stack keeps it over the app's windows like a VST3 editor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireBar {
    pub track: String,
    pub device: String,
    pub pinned: bool,
    pub stack: Vec<u64>,
    pub stack_base: u64,
    pub raise_ticket: u64,
}

impl From<service::EditorBar> for WireBar {
    fn from(b: service::EditorBar) -> Self {
        Self {
            track: b.track,
            device: b.device,
            pinned: b.pinned,
            stack: b.stack,
            stack_base: b.stack_base,
            raise_ticket: b.raise_ticket,
        }
    }
}

impl From<WireBar> for service::EditorBar {
    fn from(b: WireBar) -> Self {
        Self {
            track: b.track,
            device: b.device,
            pinned: b.pinned,
            stack: b.stack,
            stack_base: b.stack_base,
            raise_ticket: b.raise_ticket,
            ara: true,
            ..Self::default()
        }
    }
}

/// A press on the ARA editor's title bar, or the window coming forward (UI-380). The bar has
/// only the pin, so the other actions do not cross.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WireAction {
    Pin,
    Touched { window: u64, time: u64 },
    Raised(u64),
}

impl WireAction {
    pub fn from_action(a: service::EditorAction) -> Option<Self> {
        match a {
            service::EditorAction::Pin => Some(Self::Pin),
            service::EditorAction::EditorTouched { window, time } => {
                Some(Self::Touched { window, time })
            }
            service::EditorAction::EditorRaised(ticket) => Some(Self::Raised(ticket)),
            _ => None,
        }
    }
}

impl From<WireAction> for service::EditorAction {
    fn from(a: WireAction) -> Self {
        match a {
            WireAction::Pin => Self::Pin,
            WireAction::Touched { window, time } => Self::EditorTouched { window, time },
            WireAction::Raised(ticket) => Self::EditorRaised(ticket),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct WirePlacement {
    pub area: Option<WireArea>,
    pub owner: Option<isize>,
    pub at: Option<(i32, i32)>,
    pub top: bool,
    pub caption: Option<WireCaption>,
}

type WireArea = (isize, [i32; 4], [i32; 4], Option<(i32, i32)>);

impl From<service::Placement> for WirePlacement {
    fn from(p: service::Placement) -> Self {
        Self {
            area: p.area.map(|a| (a.main_window, a.main, a.work, a.restore)),
            owner: p.owner,
            at: p.at,
            top: p.top,
            caption: p.caption.map(Into::into),
        }
    }
}

impl From<WirePlacement> for service::Placement {
    fn from(p: WirePlacement) -> Self {
        Self {
            area: p.area.map(
                |(main_window, main, work, restore)| xirius_vst3::EditorArea {
                    main_window,
                    main,
                    work,
                    restore,
                    replacing: false,
                    retiring: None,
                },
            ),
            owner: p.owner,
            at: p.at,
            top: p.top,
            caption: p.caption.map(Into::into),
        }
    }
}

pub fn send<W: Write, T: Serialize>(out: &mut W, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ARA control message too large",
        ));
    }
    out.write_all(&(bytes.len() as u32).to_le_bytes())?;
    out.write_all(&bytes)?;
    out.flush()
}

pub fn recv<R: Read, T: for<'a> Deserialize<'a>>(input: &mut R) -> io::Result<T> {
    let mut size = [0; 4];
    input.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ARA control message too large",
        ));
    }
    let mut bytes = vec![0; size];
    input.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Runs only in the plugin-host executable. The worker owns all ARA callbacks and renderers.
pub fn run_worker() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let Command::Open {
        protocol,
        format,
        path,
        id,
        name,
        archive,
        fixture,
    } = recv(&mut input)?
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ARA worker needs Open",
        ));
    };
    if protocol != PROTOCOL {
        send(&mut output, &Reply::Error("ARA protocol mismatch".into()))?;
        return Ok(());
    }
    let stalled_poll_ms = fixture.map_or(0, |config| config.stalled_poll_ms);
    let limit = fixture.map_or(service::CALL_LIMIT, |config| {
        config.watch_or(service::CALL_LIMIT)
    });
    let service_id = id.clone();
    let loader: service::Loader = match format.as_str() {
        "vst3" => Box::new(move || {
            let b = crate::vst3::Vst3Binary::load(std::path::Path::new(&path), Some(&name))?;
            Ok(Box::new(b) as Box<dyn service::Binary>)
        }),
        "clap" => Box::new(move || {
            let b = crate::clap::ClapBinary::load(std::path::Path::new(&path), &id)?;
            Ok(Box::new(b) as Box<dyn service::Binary>)
        }),
        "fake" => {
            let fixture = fixture.unwrap_or_default();
            std::thread::sleep(std::time::Duration::from_millis(fixture.open_reply_ms));
            crate::fake::loader(fixture)
        }
        _ => {
            send(&mut output, &Reply::Error("unsupported ARA format".into()))?;
            return Ok(());
        }
    };
    let service =
        service::Service::start_shared_with(&service_id, loader, archive, Arc::new(|| {}), limit);
    send(&mut output, &Reply::Ack)?;
    let mut sources: HashMap<String, (Arc<SharedAudio>, Arc<SourceAudio>)> = HashMap::new();
    let mut renders: HashMap<u64, (Arc<Rendered>, String, SharedAudio)> = HashMap::new();
    let mut last_analysis = None;
    let mut number = 0_u64;
    let mut sent_loaded = false;
    while let Ok(command) = recv::<_, Command>(&mut input) {
        let reply = match command {
            Command::Open { .. } => Reply::Error("ARA worker already open".into()),
            Command::Sync(song) => {
                let mut active: HashMap<String, (Arc<SharedAudio>, Arc<SourceAudio>)> =
                    HashMap::new();
                let clips = song
                    .clips
                    .into_iter()
                    .map(|c| {
                        let pair = if let Some(pair) = sources.remove(&c.mapping) {
                            pair
                        } else if let Some(pair) = active.get(&c.mapping) {
                            pair.clone()
                        } else {
                            let mapped = Arc::new(SharedAudio::open(&c.mapping)?);
                            let audio = Arc::new(SourceAudio::mapped(mapped.clone()));
                            (mapped, audio)
                        };
                        let audio = pair.1.clone();
                        active.insert(c.mapping, pair);
                        Ok(service::ClipSpec {
                            regions: c.regions,
                            key: c.key,
                            clone_of: c.clone_of,
                            source: c.source,
                            name: c.name,
                            audio,
                            start: c.start,
                            offset: c.offset,
                            duration: c.duration,
                        })
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                sources = active;
                service.sync(&Song {
                    tempos: song.tempos,
                    bars: song.bars,
                    bpm: song.bpm,
                    numerator: song.numerator,
                    denominator: song.denominator,
                    clips,
                });
                Reply::Ack
            }
            Command::Poll => {
                if sent_loaded {
                    std::thread::sleep(std::time::Duration::from_millis(stalled_poll_ms));
                }
                sent_loaded = service.loaded();
                let generation = service.generation();
                let states = service.states();
                renders.retain(|key, _| states.get(key).is_some_and(|s| s.rendered.is_some()));
                let mut wire = Vec::with_capacity(states.len());
                for (key, state) in states {
                    let rendered = if let Some(render) = state.rendered {
                        if !renders
                            .get(&key)
                            .is_some_and(|(old, _, _)| Arc::ptr_eq(old, &render))
                        {
                            number += 1;
                            let name = format!("xirius-ara-render-{}-{number}", std::process::id());
                            let mapped = SharedAudio::create(
                                &name,
                                render.rate,
                                &render.left,
                                &render.right,
                            )?;
                            renders.insert(key, (render.clone(), name, mapped));
                        }
                        renders.get(&key).map(|(_, name, _)| WireRender {
                            name: name.clone(),
                            rate: render.rate,
                            duration: render.duration,
                            offset: render.offset,
                        })
                    } else {
                        None
                    };
                    wire.push((
                        key,
                        WireState {
                            phase: state.phase,
                            progress: state.progress,
                            rendered,
                        },
                    ));
                }
                let analysis_archive = new_analysis(&mut last_analysis, service.analysis_archive());
                Reply::Snapshot(Snapshot {
                    states: wire,
                    crashed: service.crashed(),
                    failed: service.failed(),
                    loaded: service.loaded(),
                    dirty: service.take_dirty(),
                    editing: service.editing(),
                    editor_handle: service.editor_handle(),
                    editor_error: service.take_editor_error(),
                    analysis_archive,
                    requests: service.take_requests(),
                    info: service.info(),
                    generation,
                    editor_actions: service
                        .take_editor_actions()
                        .into_iter()
                        .filter_map(WireAction::from_action)
                        .collect(),
                })
            }
            Command::Store { changed } => {
                let stored = if changed {
                    service.store_changed_later()
                } else {
                    service.store_later()
                };
                let edited = stored
                    .and_then(|rx| rx.recv_timeout(std::time::Duration::from_secs(5)).ok())
                    .flatten();
                // Store can cache the analysis without producing an edited archive.
                // The save must be able to read that cache as soon as Store completes.
                let analysis = if edited.is_none() {
                    new_analysis(&mut last_analysis, service.analysis_archive())
                } else {
                    None
                };
                Reply::Archive { edited, analysis }
            }
            Command::Notes { key, modification } => {
                Reply::Notes(service.notes(key, modification, std::time::Duration::from_secs(5)))
            }
            Command::Probe => Reply::Probe(service.probe(std::time::Duration::from_secs(5))),
            Command::Edit {
                title,
                place,
                clips,
            } => match service.request_editor(&title, place.into(), &clips) {
                Ok(()) => Reply::Ack,
                Err(e) => Reply::Error(e),
            },
            Command::Caption(c) => {
                service.set_editor_caption(c.into());
                Reply::Ack
            }
            Command::Bar(b) => {
                service.set_editor_bar(b.into());
                Reply::Ack
            }
            Command::CloseEditor => {
                service.close_editor();
                Reply::Ack
            }
            Command::MarkChanged => {
                service.mark_changed();
                Reply::Ack
            }
            Command::FixtureEdit { key, gain } => {
                crate::fake::edit(&service::modification_id(key), gain);
                Reply::Ack
            }
            Command::Close => break,
        };
        send(&mut output, &reply)?;
    }
    Ok(())
}

fn new_analysis(
    last: &mut Option<service::CachedArchive>,
    current: Option<service::CachedArchive>,
) -> Option<(Vec<u8>, String)> {
    if let (Some(old), Some(new)) = (&*last, &current)
        && Arc::ptr_eq(old, new)
    {
        return None;
    }
    let archive = current.as_deref().cloned();
    *last = current;
    archive
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> WireBar {
        WireBar {
            track: "Vocal".into(),
            device: "Pitch Editor".into(),
            pinned: true,
            stack: vec![3, 2],
            stack_base: 7,
            raise_ticket: 9,
        }
    }

    fn snapshot_json(actions: &[WireAction]) -> serde_json::Value {
        serde_json::to_value(Snapshot {
            states: Vec::new(),
            crashed: None,
            failed: None,
            loaded: true,
            dirty: false,
            editing: true,
            editor_handle: 5,
            editor_error: None,
            analysis_archive: None,
            requests: Vec::new(),
            info: None,
            generation: 1,
            editor_actions: actions.to_vec(),
        })
        .unwrap()
    }

    /// UI-380: the bar crosses to the worker whole.
    #[test]
    fn a_bar_command_round_trips() {
        let bytes = serde_json::to_vec(&Command::Bar(bar())).unwrap();
        let Command::Bar(back) = serde_json::from_slice(&bytes).unwrap() else {
            panic!("not a bar");
        };
        assert_eq!(back, bar());
    }

    /// UI-380: the bar the worker gives the window is always the ARA form, and only the name,
    /// the pin and the window stack come from the app.
    #[test]
    fn a_wire_bar_is_the_ara_form() {
        let b: service::EditorBar = bar().into();
        assert!(b.ara);
        assert_eq!(
            (b.track.as_str(), b.device.as_str()),
            ("Vocal", "Pitch Editor")
        );
        assert!(b.pinned);
        assert_eq!(
            (b.stack.as_slice(), b.stack_base, b.raise_ticket),
            (&[3, 2][..], 7, 9)
        );
        assert!(b.preset.is_empty() && b.presets.is_empty() && !b.sidechain);
        let again: WireBar = b.into();
        assert_eq!(again, bar());
    }

    /// UI-380: the presses come back with the poll; an older worker's snapshot has none.
    #[test]
    fn a_snapshot_carries_the_presses_and_an_old_one_has_none() {
        let pressed = vec![
            WireAction::Pin,
            WireAction::Touched { window: 4, time: 8 },
            WireAction::Raised(2),
        ];
        let json = snapshot_json(&pressed);
        let back: Snapshot = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.editor_actions, pressed);
        let actions: Vec<service::EditorAction> =
            back.editor_actions.into_iter().map(Into::into).collect();
        assert_eq!(
            actions,
            vec![
                service::EditorAction::Pin,
                service::EditorAction::EditorTouched { window: 4, time: 8 },
                service::EditorAction::EditorRaised(2),
            ]
        );

        let mut old = json;
        old.as_object_mut().unwrap().remove("editor_actions");
        let back: Snapshot = serde_json::from_value(old).unwrap();
        assert!(back.editor_actions.is_empty());
        assert!(back.editing);
    }

    /// UI-380: the bar has only the pin; the VST3 bar's other presses do not cross.
    #[test]
    fn only_the_ara_bar_presses_cross() {
        assert_eq!(
            WireAction::from_action(service::EditorAction::Pin),
            Some(WireAction::Pin)
        );
        for a in [
            service::EditorAction::Power,
            service::EditorAction::PresetOpen,
            service::EditorAction::Channel,
            service::EditorAction::Sidechain,
            service::EditorAction::PresetNext,
        ] {
            assert_eq!(WireAction::from_action(a), None);
        }
    }
}
