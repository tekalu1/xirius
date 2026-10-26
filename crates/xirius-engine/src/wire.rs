//! The control channel between the engine and `xirius-plugin-host` (PLG-023): messages on the
//! host's stdin and stdout, each a little-endian `u32` length and that many bytes. Audio and
//! notes do not go here; they go through the shared memory of [`crate::sandbox`].
//!
//! The engine starts the host with `--protocol` [`PROTOCOL`], and the host's first message is
//! [`FromHost::Hello`] with its own number. The two must match: an `xirius-plugin-host.exe`
//! left over from an older build is told apart instead of misreading the messages.

use std::io::{self, Read, Write};

use xirius_vst3::{Caption, PluginState};

/// The version of these messages and of the shared memory's layout together. Bump it with
/// any change to either.
pub const PROTOCOL: u32 = 7;
/// Starts the encoded [`FromHost::Hello`], so a stray first message is not taken for one.
const HELLO_MAGIC: u32 = u32::from_le_bytes(*b"XPHH");

/// What the engine asks the host to do. `slot` names one plugin instance in the host.
#[derive(Clone, Debug, PartialEq)]
pub enum ToHost {
    Open(OpenSlot),
    Close {
        slot: u32,
    },
    /// Replace the plugin's state (an undo, or a state the song was given).
    SetState {
        slot: u32,
        state: PluginState,
    },
    /// Set a parameter by numeric id or title, normalised 0..1.
    SetParam {
        slot: u32,
        key: String,
        value: f64,
    },
    /// Open the plugin's window, its caption in `caption`'s colours when given.
    OpenEditor {
        slot: u32,
        title: String,
        caption: Option<Caption>,
    },
    CloseEditor {
        slot: u32,
    },
    /// The app's theme changed: give the open window the new caption colours.
    SetCaption {
        slot: u32,
        caption: Caption,
    },
    /// Read the plugin's state now and send it back as [`FromHost::State`], whether or not
    /// the plugin said it had changed. A plugin that changes its own patch in its editor
    /// does not always say so (PLG-057).
    CaptureState {
        slot: u32,
        request: u64,
    },
    /// Move a parameter the way the editor does (begin, perform each value, end). For tests.
    SimulateEdit {
        slot: u32,
        id: u32,
        values: Vec<f64>,
    },
    /// Crash (an access violation) or hang the audio thread after this many more blocks. For
    /// tests of what the engine does when a plugin fails.
    Fault {
        slot: u32,
        kind: Fault,
        after_blocks: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Crash,
    Hang,
}

/// Everything to make one instance. The engine keeps it to open the instance again after a
/// restart.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenSlot {
    pub slot: u32,
    /// The host process's generation; it is written into the shared memory when the instance
    /// is ready, so the engine can tell a fresh instance from one of a host that went down.
    pub epoch: u64,
    pub shm: String,
    pub req: String,
    pub done: String,
    pub bundle: String,
    /// The class id; `None` takes the bundle's first audio class.
    pub class: Option<[u8; 16]>,
    pub state: Option<PluginState>,
    /// Parameters by numeric id or title, applied after the state.
    pub params: Vec<(String, f64)>,
}

/// What the host tells the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum FromHost {
    /// The first message: the host's [`PROTOCOL`] and the version of its build. Its encoding
    /// never changes, so hosts of any version can be told apart.
    Hello {
        protocol: u32,
        build: String,
    },
    Opened {
        slot: u32,
        epoch: u64,
        result: Result<Opened, String>,
    },
    /// A step of the user's edit in the editor (automation writing follows these).
    Gesture {
        slot: u32,
        gesture: Gesture,
    },
    /// The plugin's state after the user's edit settled.
    State {
        slot: u32,
        state: PluginState,
    },
    /// A reply to one exact CaptureState request; unrelated edit reports never
    /// acknowledge a save, and an abandoned request cannot answer a retry.
    CapturedState {
        slot: u32,
        request: u64,
        result: Result<PluginState, String>,
    },
    /// The plugin asked for its latency to be read again (`kLatencyChanged`, ENG-011).
    Latency {
        slot: u32,
        latency: u32,
    },
    /// The plugin's window was shut by the person (or could not be opened). Sent before
    /// [`FromHost::Closed`] for a window shut just before the plugin was closed.
    EditorClosed {
        slot: u32,
    },
    /// The plugin was taken out of the host ([`ToHost::Close`]): nothing more comes about it.
    Closed {
        slot: u32,
    },
    PlayKey {
        slot: u32,
    },
    /// The names the plugin gives its keys changed (a state or a program was loaded, EDT-134).
    NoteNames {
        slot: u32,
        names: Vec<(u8, String)>,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opened {
    pub name: String,
    pub latency: u32,
    /// Every parameter's id and title.
    pub params: Vec<(u32, String)>,
    pub warnings: Vec<String>,
    /// The names the plugin gives its keys, by key (EDT-134). Empty when it names none.
    pub note_names: Vec<(u8, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    Begin(u32),
    Perform(u32, f64),
    End(u32),
}

impl From<xirius_vst3::Gesture> for Gesture {
    fn from(g: xirius_vst3::Gesture) -> Gesture {
        match g {
            xirius_vst3::Gesture::Begin(id) => Gesture::Begin(id),
            xirius_vst3::Gesture::Perform(id, v) => Gesture::Perform(id, v),
            xirius_vst3::Gesture::End(id) => Gesture::End(id),
        }
    }
}

/// The largest message either side accepts (a big plugin state fits well below).
const MAX_MESSAGE: usize = 256 << 20;

#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) -> &mut Out {
        self.0.push(v);
        self
    }
    fn u32(&mut self, v: u32) -> &mut Out {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Out {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f64(&mut self, v: f64) -> &mut Out {
        self.u64(v.to_bits())
    }
    fn bytes(&mut self, v: &[u8]) -> &mut Out {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
        self
    }
    fn str(&mut self, v: &str) -> &mut Out {
        self.bytes(v.as_bytes())
    }
    fn note_names(&mut self, names: &[(u8, String)]) -> &mut Out {
        self.u32(names.len() as u32);
        for (key, name) in names {
            self.u8(*key).str(name);
        }
        self
    }
    fn state(&mut self, s: &PluginState) -> &mut Out {
        self.bytes(&s.component).bytes(&s.controller)
    }
    fn caption(&mut self, c: &Caption) -> &mut Out {
        self.u8(u8::from(c.dark)).u32(c.fill).u32(c.text)
    }
}

struct In<'a>(&'a [u8]);

fn short() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "message too short")
}

impl In<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.0.len() < n {
            return Err(short());
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn str(&mut self) -> io::Result<String> {
        String::from_utf8(self.bytes()?)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))
    }
    fn caption(&mut self) -> io::Result<Caption> {
        Ok(Caption {
            dark: self.u8()? != 0,
            fill: self.u32()?,
            text: self.u32()?,
        })
    }
    fn state(&mut self) -> io::Result<PluginState> {
        Ok(PluginState {
            component: self.bytes()?,
            controller: self.bytes()?,
        })
    }
    fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> io::Result<T>) -> io::Result<Vec<T>> {
        let n = self.u32()? as usize;
        (0..n).map(|_| f(self)).collect()
    }
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("unknown {what}"))
}

impl ToHost {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Out::default();
        match self {
            ToHost::Open(s) => {
                o.u8(1).u32(s.slot).u64(s.epoch);
                o.str(&s.shm).str(&s.req).str(&s.done).str(&s.bundle);
                match &s.class {
                    Some(c) => o.u8(1).bytes(c),
                    None => o.u8(0),
                };
                match &s.state {
                    Some(st) => o.u8(1).state(st),
                    None => o.u8(0),
                };
                o.u32(s.params.len() as u32);
                for (k, v) in &s.params {
                    o.str(k).f64(*v);
                }
            }
            ToHost::Close { slot } => {
                o.u8(2).u32(*slot);
            }
            ToHost::SetState { slot, state } => {
                o.u8(3).u32(*slot).state(state);
            }
            ToHost::SetParam { slot, key, value } => {
                o.u8(4).u32(*slot).str(key).f64(*value);
            }
            ToHost::OpenEditor {
                slot,
                title,
                caption,
            } => {
                o.u8(5).u32(*slot).str(title);
                match caption {
                    Some(c) => o.u8(1).caption(c),
                    None => o.u8(0),
                };
            }
            ToHost::SetCaption { slot, caption } => {
                o.u8(10).u32(*slot).caption(caption);
            }
            ToHost::CloseEditor { slot } => {
                o.u8(6).u32(*slot);
            }
            ToHost::SimulateEdit { slot, id, values } => {
                o.u8(7).u32(*slot).u32(*id).u32(values.len() as u32);
                for v in values {
                    o.f64(*v);
                }
            }
            ToHost::Fault {
                slot,
                kind,
                after_blocks,
            } => {
                let k = match kind {
                    Fault::Crash => 0,
                    Fault::Hang => 1,
                };
                o.u8(8).u32(*slot).u8(k).u64(*after_blocks);
            }
            ToHost::CaptureState { slot, request } => {
                o.u8(9).u32(*slot).u64(*request);
            }
        }
        o.0
    }

    pub fn decode(b: &[u8]) -> io::Result<ToHost> {
        let mut i = In(b);
        Ok(match i.u8()? {
            1 => {
                let (slot, epoch) = (i.u32()?, i.u64()?);
                let (shm, req, done, bundle) = (i.str()?, i.str()?, i.str()?, i.str()?);
                let class = match i.u8()? {
                    0 => None,
                    _ => Some(i.bytes()?.try_into().map_err(|_| bad("class id length"))?),
                };
                let state = match i.u8()? {
                    0 => None,
                    _ => Some(i.state()?),
                };
                let params = i.list(|i| Ok((i.str()?, i.f64()?)))?;
                ToHost::Open(OpenSlot {
                    slot,
                    epoch,
                    shm,
                    req,
                    done,
                    bundle,
                    class,
                    state,
                    params,
                })
            }
            2 => ToHost::Close { slot: i.u32()? },
            3 => ToHost::SetState {
                slot: i.u32()?,
                state: i.state()?,
            },
            4 => ToHost::SetParam {
                slot: i.u32()?,
                key: i.str()?,
                value: i.f64()?,
            },
            5 => ToHost::OpenEditor {
                slot: i.u32()?,
                title: i.str()?,
                caption: match i.u8()? {
                    0 => None,
                    _ => Some(i.caption()?),
                },
            },
            10 => ToHost::SetCaption {
                slot: i.u32()?,
                caption: i.caption()?,
            },
            6 => ToHost::CloseEditor { slot: i.u32()? },
            7 => ToHost::SimulateEdit {
                slot: i.u32()?,
                id: i.u32()?,
                values: i.list(In::f64)?,
            },
            8 => ToHost::Fault {
                slot: i.u32()?,
                kind: match i.u8()? {
                    0 => Fault::Crash,
                    1 => Fault::Hang,
                    _ => return Err(bad("fault")),
                },
                after_blocks: i.u64()?,
            },
            9 => ToHost::CaptureState {
                slot: i.u32()?,
                request: i.u64()?,
            },
            _ => return Err(bad("message to the host")),
        })
    }
}

impl FromHost {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Out::default();
        match self {
            FromHost::Hello { protocol, build } => {
                o.u8(0).u32(HELLO_MAGIC).u32(*protocol).str(build);
            }
            FromHost::Opened {
                slot,
                epoch,
                result,
            } => {
                o.u8(1).u32(*slot).u64(*epoch);
                match result {
                    Ok(r) => {
                        o.u8(1).str(&r.name).u32(r.latency);
                        o.u32(r.params.len() as u32);
                        for (id, title) in &r.params {
                            o.u32(*id).str(title);
                        }
                        o.u32(r.warnings.len() as u32);
                        for w in &r.warnings {
                            o.str(w);
                        }
                        o.note_names(&r.note_names);
                    }
                    Err(e) => {
                        o.u8(0).str(e);
                    }
                }
            }
            FromHost::Gesture { slot, gesture } => {
                o.u8(2).u32(*slot);
                match *gesture {
                    Gesture::Begin(id) => o.u8(0).u32(id).f64(0.0),
                    Gesture::Perform(id, v) => o.u8(1).u32(id).f64(v),
                    Gesture::End(id) => o.u8(2).u32(id).f64(0.0),
                };
            }
            FromHost::State { slot, state } => {
                o.u8(3).u32(*slot).state(state);
            }
            FromHost::CapturedState {
                slot,
                request,
                result,
            } => {
                o.u8(9).u32(*slot).u64(*request);
                match result {
                    Ok(state) => {
                        o.u8(1).state(state);
                    }
                    Err(error) => {
                        o.u8(0).str(error);
                    }
                }
            }
            FromHost::Latency { slot, latency } => {
                o.u8(4).u32(*slot).u32(*latency);
            }
            FromHost::EditorClosed { slot } => {
                o.u8(5).u32(*slot);
            }
            FromHost::PlayKey { slot } => {
                o.u8(6).u32(*slot);
            }
            FromHost::NoteNames { slot, names } => {
                o.u8(7).u32(*slot).note_names(names);
            }
            FromHost::Closed { slot } => {
                o.u8(8).u32(*slot);
            }
        }
        o.0
    }

    pub fn decode(b: &[u8]) -> io::Result<FromHost> {
        let mut i = In(b);
        Ok(match i.u8()? {
            0 => {
                if i.u32()? != HELLO_MAGIC {
                    return Err(bad("greeting"));
                }
                FromHost::Hello {
                    protocol: i.u32()?,
                    build: i.str()?,
                }
            }
            1 => {
                let (slot, epoch) = (i.u32()?, i.u64()?);
                let result = match i.u8()? {
                    0 => Err(i.str()?),
                    _ => Ok(Opened {
                        name: i.str()?,
                        latency: i.u32()?,
                        params: i.list(|i| Ok((i.u32()?, i.str()?)))?,
                        warnings: i.list(In::str)?,
                        note_names: i.list(|i| Ok((i.u8()?, i.str()?)))?,
                    }),
                };
                FromHost::Opened {
                    slot,
                    epoch,
                    result,
                }
            }
            2 => {
                let slot = i.u32()?;
                let (kind, id, v) = (i.u8()?, i.u32()?, i.f64()?);
                let gesture = match kind {
                    0 => Gesture::Begin(id),
                    1 => Gesture::Perform(id, v),
                    2 => Gesture::End(id),
                    _ => return Err(bad("gesture")),
                };
                FromHost::Gesture { slot, gesture }
            }
            3 => FromHost::State {
                slot: i.u32()?,
                state: i.state()?,
            },
            4 => FromHost::Latency {
                slot: i.u32()?,
                latency: i.u32()?,
            },
            5 => FromHost::EditorClosed { slot: i.u32()? },
            6 => FromHost::PlayKey { slot: i.u32()? },
            7 => FromHost::NoteNames {
                slot: i.u32()?,
                names: i.list(|i| Ok((i.u8()?, i.str()?)))?,
            },
            8 => FromHost::Closed { slot: i.u32()? },
            9 => FromHost::CapturedState {
                slot: i.u32()?,
                request: i.u64()?,
                result: if i.u8()? == 0 {
                    Err(i.str()?)
                } else {
                    Ok(i.state()?)
                },
            },
            _ => return Err(bad("message from the host")),
        })
    }
}

/// Writes one message.
pub fn send(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// Reads one message; `None` at the end of the stream.
pub fn recv(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a message of {n} bytes"),
        ));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

#[cfg(test)]
mod tests {
    fn caption() -> Caption {
        Caption {
            dark: true,
            fill: 0x121318,
            text: 0xf4f6ff,
        }
    }

    use super::*;

    #[test]
    fn messages_survive_the_trip() {
        let state = PluginState {
            component: vec![1, 2, 3],
            controller: vec![],
        };
        let to = [
            ToHost::Open(OpenSlot {
                slot: 3,
                epoch: 7,
                shm: "a".into(),
                req: "b".into(),
                done: "c".into(),
                bundle: "D:/x.vst3".into(),
                class: Some([9; 16]),
                state: Some(state.clone()),
                params: vec![("Gain".into(), 0.25), ("7".into(), 1.0)],
            }),
            ToHost::Close { slot: 1 },
            ToHost::SetState {
                slot: 2,
                state: state.clone(),
            },
            ToHost::SetParam {
                slot: 2,
                key: "Mix".into(),
                value: 0.5,
            },
            ToHost::OpenEditor {
                slot: 4,
                title: "Gain — トラック 1".into(),
                caption: None,
            },
            ToHost::OpenEditor {
                slot: 4,
                title: "Gain — トラック 1".into(),
                caption: Some(caption()),
            },
            ToHost::SetCaption {
                slot: 4,
                caption: caption(),
            },
            ToHost::CloseEditor { slot: 4 },
            ToHost::CaptureState {
                slot: 4,
                request: 23,
            },
            ToHost::SimulateEdit {
                slot: 4,
                id: 1,
                values: vec![0.1, 0.2],
            },
            ToHost::Fault {
                slot: 4,
                kind: Fault::Hang,
                after_blocks: 10,
            },
        ];
        for m in to {
            assert_eq!(ToHost::decode(&m.encode()).unwrap(), m);
        }
        let from = [
            FromHost::Hello {
                protocol: PROTOCOL,
                build: "1.0.0".into(),
            },
            FromHost::Opened {
                slot: 1,
                epoch: 2,
                result: Ok(Opened {
                    name: "Gain".into(),
                    latency: 64,
                    params: vec![(1, "Gain".into())],
                    warnings: vec!["w".into()],
                    note_names: vec![(36, "Kick".into())],
                }),
            },
            FromHost::Opened {
                slot: 1,
                epoch: 2,
                result: Err("no".into()),
            },
            FromHost::Gesture {
                slot: 1,
                gesture: Gesture::Perform(3, 0.75),
            },
            FromHost::Gesture {
                slot: 1,
                gesture: Gesture::End(3),
            },
            FromHost::CapturedState {
                slot: 1,
                request: 24,
                result: Ok(state.clone()),
            },
            FromHost::CapturedState {
                slot: 1,
                request: 25,
                result: Err("状態を取得できません".into()),
            },
            FromHost::State { slot: 1, state },
            FromHost::Latency {
                slot: 1,
                latency: 128,
            },
            FromHost::EditorClosed { slot: 9 },
            FromHost::Closed { slot: 9 },
            FromHost::PlayKey { slot: 9 },
            FromHost::NoteNames {
                slot: 9,
                names: vec![(36, "Kick".into()), (38, "Snare".into())],
            },
        ];
        for m in from {
            assert_eq!(FromHost::decode(&m.encode()).unwrap(), m);
        }
    }

    #[test]
    fn frames_are_read_back_one_by_one() {
        let mut buf = Vec::new();
        send(&mut buf, b"one").unwrap();
        send(&mut buf, b"").unwrap();
        let mut r = &buf[..];
        assert_eq!(recv(&mut r).unwrap().unwrap(), b"one");
        assert_eq!(recv(&mut r).unwrap().unwrap(), b"");
        assert!(recv(&mut r).unwrap().is_none());
    }
}
