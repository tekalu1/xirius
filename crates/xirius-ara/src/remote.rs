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
pub const PROTOCOL: u32 = 1;

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
    Archive(Option<(Vec<u8>, String)>),
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
    pub editor_error: Option<String>,
    pub analysis_archive: Option<(Vec<u8>, String)>,
    pub requests: Vec<Request>,
    pub info: Option<FactoryInfo>,
    pub generation: u64,
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
    let service = service::Service::start_shared(&service_id, loader, archive, Arc::new(|| {}));
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
                let analysis = service.analysis_archive();
                let analysis_archive = match (&last_analysis, &analysis) {
                    (Some(old), Some(new)) if Arc::ptr_eq(old, new) => None,
                    _ => analysis.as_ref().map(|a| (**a).clone()),
                };
                last_analysis = analysis;
                Reply::Snapshot(Snapshot {
                    states: wire,
                    crashed: service.crashed(),
                    failed: service.failed(),
                    loaded: service.loaded(),
                    dirty: service.take_dirty(),
                    editing: service.editing(),
                    editor_error: service.take_editor_error(),
                    analysis_archive,
                    requests: service.take_requests(),
                    info: service.info(),
                    generation: service.generation(),
                })
            }
            Command::Store { changed } => {
                let stored = if changed {
                    service.store_changed_later()
                } else {
                    service.store_later()
                };
                Reply::Archive(
                    stored
                        .and_then(|rx| rx.recv_timeout(std::time::Duration::from_secs(5)).ok())
                        .flatten(),
                )
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
