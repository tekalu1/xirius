//! The mixer in the lower drawer (MIX-001, MIX-002, MIX-004, MIX-015, MIX-016,
//! MIX-017, MIX-018, MIX-022, MIX-025, MIX-026, UI-021, UI-040, PLG-021, PLG-051).
//!
//! One strip per track, then the buses, then the master (mock v4). Down a
//! strip runs the sound's way: the input, the insert rows, the fader with its
//! pan and buttons, the send rows, the output. Every change is a core
//! command: a fader, a pan knob or a send knob is heard while it is dragged
//! (the player is handed an edited copy of the song) and becomes one step of
//! the history when it is let go.

pub mod bench;
pub(crate) mod editors;
pub mod knob;
pub mod meters;
pub(crate) mod paint;
pub mod scan;

pub use paint::paint;

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;
use xirius_core::id::{PluginId, SendId, TrackId};
use xirius_core::model::{AuxSend, MIN_DB, Output, PluginKind, Project, Track, TrackKind};
use xirius_plugins::Catalog;
use xirius_session::autowrite::Phase;
use xirius_session::host::Strip;

use crate::popup::{Choice, PopupKind};
use crate::studio::{DrawerTab, Geometry, Hit, Rect, Studio};
use crate::tree::RowKind;
use editors::OpenEditor;
use meters::{FLOOR_DB, Meters, db_to_t, t_to_db};

pub(crate) fn send_level_db(send: &AuxSend) -> f64 {
    send.muted_level.unwrap_or(send.level_db)
}

/// Height of the drawer's content when it shows the mixer: strips of the
/// mock's 340 px with their gap above and below.
pub const BODY_H: f32 = 344.0;
pub const STRIP_W: f32 = 116.0;
pub const STRIP_GAP: f32 = 2.0;
const PAD: f32 = 6.0;
const GAP: f32 = 5.0;
pub const ROW_H: f32 = 22.0;
const ROW_GAP: f32 = 2.0;
const NAME_H: f32 = 18.0;
/// Insert rows shown at once at most (the wheel moves the list on).
pub const INSERT_ROWS: usize = 3;
pub const SEND_ROWS: usize = 2;
const ICON_W: f32 = 12.0;
const ICON_GAP: f32 = 6.0;
const LEFT_W: f32 = 38.0;
const KNOB: f32 = 30.0;
const BTN_W: f32 = 24.0;
const BTN: f32 = 20.0;
const BTN_GAP: f32 = 4.0;
const FADER_W: f32 = 26.0;
/// Between the fader and its meters: what is left of the column.
const FADER_GAP: f32 = 4.0;
const METER_W: f32 = 5.0;
const METER_GAP: f32 = 2.0;
const DB_H: f32 = 14.0;
const SEND_KNOB: f32 = 20.0;
const PP_W: f32 = 18.0;
/// The part of an insert row that bypasses the plugin.
const BYPASS_W: f32 = 20.0;
/// The fader's cap: the travel is the fader's height less this.
pub const CAP_H: f32 = 12.0;
/// Pixels of drag from one end of a knob to the other.
const KNOB_TRAVEL: f32 = 150.0;
/// The space between the channels and the master (the mock's `.mixer` gap).
const MASTER_GAP: f32 = 6.0;
/// The scroll bar under the channels, the space above it, and the thumb's least length.
pub const BAR_H: f32 = 8.0;
const BAR_GAP: f32 = 4.0;
const BAR_MIN: f32 = 24.0;

/// What is under the pointer in the mixer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerHit {
    /// A part of a strip that only selects it.
    Strip(TrackId),
    Fader(TrackId),
    Pan(TrackId),
    Mute(TrackId),
    Solo(TrackId),
    Arm(TrackId),
    /// The dot at the start of an insert row: bypass on and off.
    Bypass(TrackId, PluginId),
    /// The rest of an insert row: the plugin's own window.
    Open(TrackId, PluginId),
    AddInsert(TrackId),
    /// The ＋ row of an instrument track without an instrument (PLG-019).
    AddInstrument(TrackId),
    SendLevel(TrackId, SendId),
    /// The glyph that sends from before or after the fader.
    SendPre(TrackId, SendId),
    AddSend(TrackId),
    Output(TrackId),
    /// The input row of an audio track: the device it records (REC-017).
    Input(TrackId),
}

impl MixerHit {
    pub fn track(self) -> TrackId {
        match self {
            MixerHit::Strip(t)
            | MixerHit::Fader(t)
            | MixerHit::Pan(t)
            | MixerHit::Mute(t)
            | MixerHit::Solo(t)
            | MixerHit::Arm(t)
            | MixerHit::Bypass(t, _)
            | MixerHit::Open(t, _)
            | MixerHit::AddInsert(t)
            | MixerHit::AddInstrument(t)
            | MixerHit::SendLevel(t, _)
            | MixerHit::SendPre(t, _)
            | MixerHit::AddSend(t)
            | MixerHit::Output(t)
            | MixerHit::Input(t) => t,
        }
    }
}

/// Where the parts of one strip sit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripLayout {
    pub track: TrackId,
    pub kind: TrackKind,
    pub rect: Rect,
    pub name: Rect,
    /// The input row (tracks only).
    pub input: Option<Rect>,
    /// The insert list: rows of [`ROW_H`].
    pub inserts: Rect,
    pub pan: Option<Rect>,
    pub mute: Option<Rect>,
    pub solo: Option<Rect>,
    pub arm: Option<Rect>,
    /// The fader's column; the cap travels its height less [`CAP_H`].
    pub fader: Rect,
    pub meter: [Rect; 2],
    pub db: Rect,
    /// The send list (not on the master).
    pub sends: Option<Rect>,
    pub output: Option<Rect>,
    pub rows: Rows,
}

/// The rows the insert and send lists show: as many as the fullest strip
/// needs with its ＋ row, up to [`INSERT_ROWS`] and [`SEND_ROWS`]. The same
/// for every strip, so the faders line up; what the lists do not need goes
/// to the faders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rows {
    pub inserts: usize,
    pub sends: usize,
}

impl Rows {
    pub const MAX: Rows = Rows {
        inserts: INSERT_ROWS,
        sends: SEND_ROWS,
    };

    pub fn of(p: &Project) -> Rows {
        let most = |n: fn(&Track) -> usize| p.tracks.values().map(|t| n(t) + 1).max().unwrap_or(1);
        Rows {
            inserts: most(|t| t.inserts.len() + crate::instrument::gap(t)).clamp(1, INSERT_ROWS),
            sends: most(|t| if t.id.is_master() { 0 } else { t.sends.len() }).clamp(1, SEND_ROWS),
        }
    }

    fn of_list(self, list: List) -> usize {
        match list {
            List::Inserts => self.inserts,
            List::Sends => self.sends,
        }
    }
}

fn list_h(rows: usize) -> f32 {
    rows as f32 * ROW_H + rows.saturating_sub(1) as f32 * ROW_GAP
}

/// Row `i` of a list.
pub fn list_row(list: Rect, i: usize) -> Rect {
    Rect::new(list.x, list.y + i as f32 * (ROW_H + ROW_GAP), list.w, ROW_H)
}

/// The row of a list under `y`, if `y` is on a row and not in a gap.
pub(crate) fn row_at(list: Rect, y: f32) -> Option<usize> {
    let i = ((y - list.y) / (ROW_H + ROW_GAP)).floor();
    if i < 0.0 {
        return None;
    }
    let i = i as usize;
    list_row(list, i).contains(list.x, y).then_some(i)
}

/// The small icon that names a section of the strip, left of its rows.
pub fn section_icon(row: Rect) -> Rect {
    Rect::new(
        row.x - ICON_W - ICON_GAP,
        row.y + (ROW_H - ICON_W) / 2.0,
        ICON_W,
        ICON_W,
    )
}

/// A send row's parts: the bus's name, the level knob and the pre/post glyph.
pub fn send_parts(row: Rect) -> (Rect, Rect, Rect) {
    let pp = Rect::new(row.right() - PP_W, row.y, PP_W, row.h);
    let knob = Rect::new(
        pp.x - 2.0 - SEND_KNOB,
        row.y + (row.h - SEND_KNOB) / 2.0,
        SEND_KNOB,
        SEND_KNOB,
    );
    let name = Rect::new(row.x, row.y, knob.x - 2.0 - row.x, row.h);
    (name, knob, pp)
}

/// An insert row's bypass dot.
pub fn bypass_part(row: Rect) -> Rect {
    Rect::new(row.x, row.y, BYPASS_W, row.h)
}

impl StripLayout {
    pub fn new(
        track: TrackId,
        kind: TrackKind,
        x: f32,
        top: f32,
        h: f32,
        rows: Rows,
    ) -> StripLayout {
        Self::sized(track, kind, x, top, h, rows, STRIP_W)
    }

    pub fn sized(
        track: TrackId,
        kind: TrackKind,
        x: f32,
        top: f32,
        h: f32,
        rows: Rows,
        width: f32,
    ) -> StripLayout {
        let rect = Rect::new(x, top, width, h);
        let inner = Rect::new(
            x + PAD,
            top + PAD,
            width - PAD * 2.0,
            (h - PAD * 2.0).max(0.0),
        );
        let body_x = inner.x + ICON_W + ICON_GAP;
        let body_w = inner.right() - body_x;
        let master = kind == TrackKind::Master;
        let bus = kind == TrackKind::Bus;
        let mut y = inner.y;
        let name = Rect::new(inner.x, y, inner.w, NAME_H);
        y += NAME_H + GAP;
        let input = Rect::new(body_x, y, body_w, ROW_H);
        y += ROW_H + GAP;
        let inserts = Rect::new(body_x, y, body_w, list_h(rows.inserts));
        y += inserts.h + GAP;
        // From the bottom: the output, the sends; the faders take what is left,
        // so they line up across strips of every kind.
        let output = Rect::new(body_x, inner.bottom() - ROW_H, body_w, ROW_H);
        let sends = Rect::new(
            body_x,
            output.y - GAP - list_h(rows.sends),
            body_w,
            list_h(rows.sends),
        );
        // The master has no sends: its fader reaches the bottom.
        let bottom = if master {
            inner.bottom()
        } else {
            sends.y - GAP
        };
        // Like the lists, the pan and the fader have their section's icon left
        // of them (mock v4's `.cfpw`).
        let zone = Rect::new(body_x, y, body_w, (bottom - y).max(0.0));
        let left = !master;
        let btn = |i: f32| {
            Rect::new(
                zone.x + (LEFT_W - BTN_W) / 2.0,
                zone.y + KNOB + BTN_GAP + i * (BTN + BTN_GAP),
                BTN_W,
                BTN,
            )
        };
        let faders = if left {
            Rect::new(
                zone.x + LEFT_W + ICON_GAP,
                zone.y,
                zone.w - LEFT_W - ICON_GAP,
                zone.h,
            )
        } else {
            zone
        };
        let group = FADER_W + FADER_GAP + METER_W * 2.0 + METER_GAP;
        let gx = faders.x + ((faders.w - group) / 2.0).max(0.0);
        let travel_h = (zone.h - DB_H - 2.0).max(0.0);
        let mx = gx + FADER_W + FADER_GAP;
        StripLayout {
            track,
            kind,
            rect,
            name,
            input: (!master && !bus).then_some(input),
            inserts,
            pan: left.then(|| Rect::new(zone.x + (LEFT_W - KNOB) / 2.0, zone.y, KNOB, KNOB)),
            mute: left.then(|| btn(0.0)),
            solo: left.then(|| btn(1.0)),
            arm: (!master && !bus).then(|| btn(2.0)),
            fader: Rect::new(gx, zone.y, FADER_W, travel_h),
            meter: [
                Rect::new(mx, zone.y, METER_W, travel_h),
                Rect::new(mx + METER_W + METER_GAP, zone.y, METER_W, travel_h),
            ],
            db: Rect::new(faders.x, zone.y + travel_h + 2.0, faders.w, DB_H),
            sends: (!master).then_some(sends),
            output: Some(output),
            rows,
        }
    }

    /// The row where the section of the pan and the fader names itself.
    pub fn level_row(&self) -> Rect {
        let y = self.pan.map_or(self.fader.y, |r| r.y);
        Rect::new(
            self.inserts.x,
            y + (KNOB - ROW_H) / 2.0,
            self.inserts.w,
            ROW_H,
        )
    }

    /// Where the middle of the fader's cap is for a level in dB.
    pub fn cap_y(&self, db: f64) -> f32 {
        let t = db_to_t(db as f32);
        self.fader.bottom() - CAP_H / 2.0 - t * (self.fader.h - CAP_H)
    }

    /// The meter's channel of this strip.
    pub fn strip(&self) -> Strip {
        if self.kind == TrackKind::Master {
            Strip::Master
        } else {
            Strip::Track(self.track)
        }
    }
}

/// The strips in mixer order: the tracks as the column shows them, the buses,
/// the master.
pub fn strip_order(p: &Project) -> Vec<TrackId> {
    let ordered = crate::draw::ordered_tracks(p);
    let mut out: Vec<TrackId> = ordered
        .iter()
        .filter(|t| matches!(t.kind, TrackKind::Audio | TrackKind::Instrument))
        .map(|t| t.id)
        .collect();
    out.extend(
        ordered
            .iter()
            .filter(|t| t.kind == TrackKind::Bus)
            .map(|t| t.id),
    );
    for t in p.tracks.values() {
        if t.kind == TrackKind::Bus && !out.contains(&t.id) {
            out.push(t.id);
        }
    }
    if p.tracks.contains_key(&TrackId::MASTER) {
        out.push(TrackId::MASTER);
    }
    out
}

/// The two lists of a strip that scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum List {
    Inserts,
    Sends,
}

/// A fader or a knob being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub hit: MixerHit,
    y0: f32,
    /// Pixels from one end to the other.
    travel: f32,
    start: f64,
    /// The value shown and heard now.
    pub value: f64,
}

/// The mixer's own state; the levels and the routing are the song's.
#[derive(Default)]
pub struct Mixer {
    pub meters: Meters,
    /// Distance the channels are scrolled right, in px.
    pub scroll: f32,
    /// The scroll bar's thumb is held: how far the pointer is right of its left edge.
    pub bar_grab: Option<f32>,
    pub drag: Option<Drag>,
    /// When a controller's knob last moved the drag in progress (OPS-007); none for a pointer's.
    pub knob_at: Option<std::time::Instant>,
    pub fx_drag: Option<crate::fx_chain::Drag>,
    /// A channel name held, on its way to another channel.
    pub name_drag: Option<crate::name_drag::Drag>,
    /// The master, when it is the strip pressed last (a selected row never is the master).
    pub master_picked: bool,
    pub recent: Vec<crate::instrument::Pick>,
    pub preset_panel: Option<crate::fx_preset::Panel>,
    pub browser_fx_focus: bool,
    /// A file supplied by a caller in place of the system's preset picker.
    pub preset_file: Option<std::path::PathBuf>,
    offsets: HashMap<(TrackId, List), usize>,
    pub(crate) editors: Vec<OpenEditor>,
    /// The plugins found by the last scan; read when a list first needs it.
    pub catalog: Option<Arc<Catalog>>,
    pub scan: scan::PluginScan,
    /// How long the last paint took, in µs (for the measurement of UI-024).
    pub paint_us: Cell<u64>,
    /// The channel a double click on a track's head brought up, and when it did (ms).
    flash: Option<(TrackId, u64)>,
}

/// How long the channel brought up by `mixer.focus` is lit, and how long it stays at full.
pub const FLASH_MS: u64 = 1000;
const FLASH_HOLD_MS: u64 = 350;

impl Mixer {
    /// Lights `t`'s strip, from `now`.
    pub(crate) fn flash_strip(&mut self, t: TrackId, now: u64) {
        self.flash = Some((t, now));
    }

    /// How lit `t`'s strip is at `now`: 1 right after the call, down to 0 as it fades.
    pub fn flash_level(&self, t: TrackId, now: u64) -> f32 {
        let Some((id, from)) = self.flash else {
            return 0.0;
        };
        let e = now.saturating_sub(from);
        if id != t || e >= FLASH_MS {
            0.0
        } else if e <= FLASH_HOLD_MS {
            1.0
        } else {
            let left = 1.0 - (e - FLASH_HOLD_MS) as f32 / (FLASH_MS - FLASH_HOLD_MS) as f32;
            left * left
        }
    }

    /// Lets the light go once it has faded; true when it did.
    pub(crate) fn flash_expire(&mut self, now: u64) -> bool {
        match self.flash {
            Some((_, from)) if now.saturating_sub(from) >= FLASH_MS => {
                self.flash = None;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn set_insert_offset(&mut self, t: TrackId, offset: usize) {
        self.offsets.insert((t, List::Inserts), offset);
    }
    /// The first row a list shows.
    pub fn offset(&self, t: TrackId, list: List) -> usize {
        self.offsets.get(&(t, list)).copied().unwrap_or(0)
    }

    /// A plugin's window is open.
    pub fn is_open(&self, id: PluginId) -> bool {
        self.editors.iter().any(|e| e.id == id)
    }

    pub fn open_editors(&self) -> usize {
        self.editors.len()
    }

    /// Takes the windows `which` picks out of the list (they shut when dropped).
    pub(crate) fn take_editors(&mut self, which: impl Fn(&OpenEditor) -> bool) -> Vec<OpenEditor> {
        let (gone, kept) = std::mem::take(&mut self.editors)
            .into_iter()
            .partition(|e| which(e));
        self.editors = kept;
        gone
    }

    /// The handle of a plugin's window in this process (for tests and screenshots).
    pub fn editor_hwnd(&self, id: PluginId) -> Option<isize> {
        self.editors
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.editor())
            .map(|e| e.hwnd())
    }

    /// Runs the thread's messages for a while, so the windows draw where no GUI loop runs them
    /// (tests).
    pub fn pump_editors(&mut self, duration: std::time::Duration) {
        if let Some(e) = self.editors.iter_mut().find_map(|e| e.editor_mut()) {
            e.pump(duration);
        }
    }

    /// What is turned in the plugin's open window is heard (it shows the instance that plays).
    pub fn editor_heard(&self, id: PluginId) -> Option<bool> {
        self.editors.iter().find(|e| e.id == id).map(|e| e.heard())
    }

    /// The song changed: windows of plugins that are gone shut, lists keep
    /// their ＋ row in reach.
    fn follow(&mut self, p: &Project) {
        let rows = Rows::of(p);
        let live = |id: PluginId| {
            p.tracks
                .values()
                .any(|t| t.inserts.iter().any(|x| x.id == id))
        };
        self.editors.retain(|e| live(e.id));
        self.offsets.retain(|(t, list), off| {
            let Some(track) = p.tracks.get(t) else {
                return false;
            };
            *off = (*off).min(list_max(track, *list, rows));
            true
        });
        if let Some(d) = self.drag
            && !p.tracks.contains_key(&d.hit.track())
        {
            self.drag = None;
        }
    }
}

/// How far a list can scroll: its rows and the ＋ row, less what fits.
fn list_max(track: &Track, list: List, rows: Rows) -> usize {
    let n = match list {
        List::Inserts => track.inserts.len() + crate::instrument::gap(track),
        List::Sends => track.sends.len(),
    };
    (n + 1).saturating_sub(rows.of_list(list))
}

/// The song with `hit` set to `value`: what the player hears during a drag.
fn apply(p: &mut Project, hit: MixerHit, value: f64) {
    let Ok(t) = p.track_mut(hit.track()) else {
        return;
    };
    match hit {
        MixerHit::Fader(_) => t.volume_db = value,
        MixerHit::Pan(_) => t.pan = value,
        MixerHit::SendLevel(_, s) => {
            if let Some(send) = t.sends.iter_mut().find(|x| x.id == s) {
                if send.muted_level.is_some() {
                    send.muted_level = Some(value);
                } else {
                    send.level_db = value;
                }
            }
        }
        _ => {}
    }
}

/// The routing stays free of loops with `t`'s output set to `to`.
fn output_ok(p: &Project, t: TrackId, to: Option<Output>) -> bool {
    let mut q = p.clone();
    match q.track_mut(t) {
        Ok(track) => track.output = to,
        Err(_) => return false,
    }
    q.check_routing().is_ok()
}

/// The routing stays free of loops with a send from `t` to `to` added.
pub(crate) fn send_ok(p: &Project, t: TrackId, to: TrackId) -> bool {
    let mut q = p.clone();
    match q.track_mut(t) {
        Ok(track) => track.sends.push(AuxSend {
            id: SendId(u64::MAX),
            to,
            level_db: 0.0,
            pre: false,
            muted_level: None,
        }),
        Err(_) => return false,
    }
    q.check_routing().is_ok()
}

/// The buses in strip order.
fn buses(p: &Project) -> Vec<&Track> {
    strip_order(p)
        .into_iter()
        .filter_map(|id| p.tracks.get(&id).map(|t| &**t))
        .filter(|t| t.kind == TrackKind::Bus)
        .collect()
}

/// The places a track can play to: the folder's way, the master and the
/// buses, without the ones that would feed the track back into itself.
pub fn output_choices(p: &Project, t: TrackId) -> Vec<Choice> {
    output_choices_with(p, t, Vec::new())
}

/// [`output_choices`] with the MIDI output `ports` an instrument track can play to
/// listed after the buses (ENG-028).
pub fn output_choices_with(p: &Project, t: TrackId, ports: Vec<String>) -> Vec<Choice> {
    let mut all = vec![
        (None, "default".to_string(), "既定".to_string()),
        (Some(Output::Master), "master".into(), "マスター".into()),
    ];
    all.extend(
        buses(p)
            .into_iter()
            .filter(|b| b.id != t)
            .map(|b| (Some(Output::Bus(b.id)), b.id.to_string(), b.name.clone())),
    );
    let mut out: Vec<Choice> = all
        .into_iter()
        .filter(|(to, _, _)| output_ok(p, t, *to))
        .map(|(_, to, label)| Choice { to, label })
        .collect();
    if let Some(track) = p.tracks.get(&t)
        && track.kind == TrackKind::Instrument
    {
        out.extend(crate::midi_out_row::port_choices(
            ports,
            track.midi_out.as_ref(),
        ));
    }
    out
}

/// The inputs the input row lists: the default, then the device's channels
/// (`In 1+2`, `In 1`, …). An input the song names that the device does not
/// have now stays in the list, so the row still shows what was chosen.
pub fn input_choices(mut names: Vec<String>, current: Option<String>) -> Vec<Choice> {
    if let Some(current) = current
        && !names.contains(&current)
    {
        names.push(current);
    }
    let mut out = vec![Choice {
        to: "none".into(),
        label: crate::input_row::default_label(),
    }];
    out.extend(names.into_iter().map(|n| Choice {
        to: n.clone(),
        label: n,
    }));
    out
}

/// The buses a new send of `t` can go to: not itself, not one it already
/// sends to, none that would make a loop.
pub fn send_choices(p: &Project, t: TrackId) -> Vec<Choice> {
    let Some(track) = p.tracks.get(&t) else {
        return Vec::new();
    };
    let mut out: Vec<Choice> = buses(p)
        .into_iter()
        .filter(|b| b.id != t && !track.sends.iter().any(|s| s.to == b.id))
        .filter(|b| send_ok(p, t, b.id))
        .map(|b| Choice {
            to: b.id.to_string(),
            label: b.name.clone(),
        })
        .collect();
    // Last, a bus made for this send (MIX-025). A bus sends nowhere new.
    if track.kind != TrackKind::Bus {
        out.push(Choice {
            to: NEW_BUS.into(),
            label: "新しいバス".into(),
        });
    }
    if out.is_empty() {
        return vec![Choice {
            to: String::new(),
            label: "送り先のバスがありません".into(),
        }];
    }
    out
}

/// The send list's choice that makes a bus and sends to it.
pub const NEW_BUS: &str = "new-bus";

/// The effects the scan found, by name.
pub fn insert_choices(catalog: &Catalog) -> Vec<Choice> {
    let out: Vec<Choice> = xirius_params::BUILTINS
        .iter()
        .filter(|d| d.kind == xirius_params::Kind::Effect)
        .map(|d| Choice {
            to: format!("builtin:{}", d.uid),
            label: format!("{} — 内蔵", d.name),
        })
        .chain(
            catalog
                .search("", Some(PluginKind::Effect))
                .into_iter()
                .map(|e| Choice {
                    to: e.uid.clone(),
                    label: if e.vendor.is_empty() {
                        e.name.clone()
                    } else {
                        format!("{} — {}", e.name, e.vendor)
                    },
                }),
        )
        .collect();
    if out.is_empty() {
        return vec![Choice {
            to: String::new(),
            label: "エフェクトが見つかりません".into(),
        }];
    }
    out
}

impl Studio {
    pub fn mixer_shown(&self) -> bool {
        self.drawer_open && self.tab == DrawerTab::Mixer
    }

    fn strip_width(&self) -> f32 {
        if self.song.session().state.view.mixer_wide {
            STRIP_W * 1.4
        } else {
            STRIP_W
        }
    }

    /// The part of the body the channels scroll in: all of it left of the master, which
    /// stays at the right edge (the master is not a channel that scrolls away).
    pub fn mixer_view(&self, g: &Geometry) -> Rect {
        let body = g.drawer_body;
        let master = self.project().tracks.contains_key(&TrackId::MASTER);
        let w = if master {
            body.w - STRIP_GAP - self.strip_width() - MASTER_GAP
        } else {
            body.w
        };
        Rect::new(body.x, body.y, w.max(0.0), body.h)
    }

    /// The strips on screen, left to right: the channels that show in the view, then the
    /// master. A channel at the view's right edge may reach under the master, so the
    /// parts of a channel count only where [`Studio::mixer_view`] holds the point.
    pub fn mixer_strips(&self, g: &Geometry) -> Vec<StripLayout> {
        let width = self.strip_width();
        let p = self.project();
        let body = g.drawer_body;
        let view = self.mixer_view(g);
        let (top, h) = (
            body.y + STRIP_GAP,
            (body.h - STRIP_GAP * 2.0 - BAR_H - BAR_GAP).max(0.0),
        );
        let rows = Rows::of(&p);
        let mut out: Vec<StripLayout> = strip_order(&p)
            .into_iter()
            .filter(|id| !id.is_master())
            .enumerate()
            .filter_map(|(i, id)| {
                let x = view.x + STRIP_GAP + i as f32 * (width + STRIP_GAP) - self.mixer.scroll;
                if x + width <= view.x || x >= view.right() {
                    return None;
                }
                let kind = p.tracks.get(&id)?.kind;
                Some(StripLayout::sized(id, kind, x, top, h, rows, width))
            })
            .collect();
        if let Some(master) = p.tracks.get(&TrackId::MASTER) {
            let x = body.right() - STRIP_GAP - width;
            out.push(StripLayout::sized(
                master.id,
                master.kind,
                x,
                top,
                h,
                rows,
                width,
            ));
        }
        out
    }

    /// Whether the point is on the strip: a channel only where the view shows it.
    pub(crate) fn strip_holds(&self, g: &Geometry, s: &StripLayout, x: f32, y: f32) -> bool {
        s.rect.contains(x, y) && (s.track.is_master() || self.mixer_view(g).contains(x, y))
    }

    /// How far the channels scroll: what does not fit left of the master.
    pub fn mixer_max_scroll(&self, g: &Geometry) -> f32 {
        let n = strip_order(&self.project())
            .iter()
            .filter(|t| !t.is_master())
            .count() as f32;
        (STRIP_GAP + n * (self.strip_width() + STRIP_GAP) - self.mixer_view(g).w).max(0.0)
    }

    /// The scroll bar under the channels: its groove and the thumb in it. None while
    /// every channel fits (the space stays, so the strips do not jump).
    pub fn mixer_bar(&self, g: &Geometry) -> Option<(Rect, Rect)> {
        if !self.mixer_shown() {
            return None;
        }
        let max = self.mixer_max_scroll(g);
        if max <= 0.0 {
            return None;
        }
        let view = self.mixer_view(g);
        let groove = Rect::new(
            view.x + STRIP_GAP,
            g.drawer_body.bottom() - STRIP_GAP - BAR_H,
            (view.w - STRIP_GAP * 2.0).max(0.0),
            BAR_H,
        );
        let len = (groove.w * view.w / (view.w + max)).clamp(BAR_MIN.min(groove.w), groove.w);
        let x = groove.x + (groove.w - len) * (self.mixer.scroll / max).clamp(0.0, 1.0);
        Some((groove, Rect::new(x, groove.y, len, BAR_H)))
    }

    /// A press on the bar: on the thumb it takes hold, elsewhere on the bar it brings the
    /// thumb under the pointer first.
    pub fn mixer_bar_down(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some((groove, thumb)) = self.mixer_bar(g) else {
            return false;
        };
        let area = Rect::new(
            groove.x,
            groove.y - BAR_GAP,
            groove.w,
            BAR_H + BAR_GAP + STRIP_GAP,
        );
        if !area.contains(x, y) {
            return false;
        }
        if !thumb.contains(x, y) {
            self.mixer_bar_to(g, x - thumb.w / 2.0);
        }
        let held = self.mixer_bar(g).map_or(0.0, |(_, t)| x - t.x);
        self.mixer.bar_grab = Some(held);
        true
    }

    /// The pointer moved while the thumb is held.
    pub fn mixer_bar_move(&mut self, g: &Geometry, x: f32) -> bool {
        let Some(held) = self.mixer.bar_grab else {
            return false;
        };
        self.mixer_bar_to(g, x - held);
        true
    }

    /// The thumb is let go. True when it was held.
    pub fn mixer_bar_up(&mut self) -> bool {
        self.mixer.bar_grab.take().is_some()
    }

    /// Puts the thumb's left edge at `x` (as far as the groove goes).
    fn mixer_bar_to(&mut self, g: &Geometry, x: f32) {
        let Some((groove, thumb)) = self.mixer_bar(g) else {
            return;
        };
        let span = groove.w - thumb.w;
        if span > 0.0 {
            let max = self.mixer_max_scroll(g);
            self.mixer.scroll = ((x - groove.x) / span).clamp(0.0, 1.0) * max;
        }
    }

    /// What is under a point of the mixer, or `None` outside its strips.
    pub fn mixer_hit(&self, g: &Geometry, x: f32, y: f32) -> Option<MixerHit> {
        if !self.mixer_shown() || !g.drawer_body.contains(x, y) {
            return None;
        }
        let p = self.project();
        let s = self
            .mixer_strips(g)
            .into_iter()
            .find(|s| self.strip_holds(g, s, x, y))?;
        let t = s.track;
        let track = p.tracks.get(&t)?;
        let on = |r: Option<Rect>| r.is_some_and(|r| r.contains(x, y));
        let hit = if on(s.pan) {
            MixerHit::Pan(t)
        } else if on(s.mute) {
            MixerHit::Mute(t)
        } else if on(s.solo) {
            MixerHit::Solo(t)
        } else if on(s.arm) {
            MixerHit::Arm(t)
        } else if s.fader.contains(x, y) {
            MixerHit::Fader(t)
        } else if s.inserts.contains(x, y) {
            crate::fx_chain::hit(self, track, s.inserts, s.rows.inserts, x, y)?
        } else if on(s.sends) {
            let list = s.sends.expect("on a send list");
            let Some(i) = row_at(list, y) else {
                return Some(MixerHit::Strip(t));
            };
            let k = self.mixer.offset(t, List::Sends) + i;
            let (_, knob, pp) = send_parts(list_row(list, i));
            match track.sends.get(k) {
                Some(send) if knob.contains(x, y) => MixerHit::SendLevel(t, send.id),
                Some(send) if pp.contains(x, y) => MixerHit::SendPre(t, send.id),
                Some(_) => MixerHit::Strip(t),
                None if k == track.sends.len() => MixerHit::AddSend(t),
                None => MixerHit::Strip(t),
            }
        } else if on(s.output) {
            MixerHit::Output(t)
        } else if on(s.input) && matches!(track.kind, TrackKind::Audio | TrackKind::Instrument) {
            MixerHit::Input(t)
        } else {
            MixerHit::Strip(t)
        };
        Some(hit)
    }

    /// The rectangle of a part of the mixer on screen.
    pub fn mixer_rect(&self, g: &Geometry, hit: MixerHit) -> Option<Rect> {
        let t = hit.track();
        let s = self.mixer_strips(g).into_iter().find(|s| s.track == t)?;
        let p = self.project();
        let track = p.tracks.get(&t)?;
        let insert_row = |k: usize| {
            let i = k.checked_sub(self.mixer.offset(t, List::Inserts))?;
            (i < s.rows.inserts).then(|| list_row(s.inserts, i))
        };
        let send_row = |k: usize| {
            let i = k.checked_sub(self.mixer.offset(t, List::Sends))?;
            let list = s.sends?;
            (i < s.rows.sends).then(|| list_row(list, i))
        };
        let gap = crate::instrument::gap(track);
        let insert = |id: PluginId| {
            track
                .inserts
                .iter()
                .position(|x| x.id == id)
                .map(|k| k + gap)
        };
        let send = |id: SendId| track.sends.iter().position(|x| x.id == id);
        match hit {
            MixerHit::Strip(_) => Some(s.name),
            MixerHit::Fader(_) => Some(s.fader),
            MixerHit::Pan(_) => s.pan,
            MixerHit::Mute(_) => s.mute,
            MixerHit::Solo(_) => s.solo,
            MixerHit::Arm(_) => s.arm,
            MixerHit::Bypass(_, id) => insert_row(insert(id)?).map(bypass_part),
            MixerHit::Open(_, id) if self.health.crashed(id) => insert_row(insert(id)?),
            MixerHit::Open(_, id) => {
                insert_row(insert(id)?).map(|r| Rect::new(r.x + BYPASS_W, r.y, r.w - BYPASS_W, r.h))
            }
            MixerHit::AddInsert(_) => insert_row(track.inserts.len() + gap),
            MixerHit::AddInstrument(_) => (gap == 1).then(|| insert_row(0)).flatten(),
            MixerHit::SendLevel(_, id) => send_row(send(id)?).map(|r| send_parts(r).1),
            MixerHit::SendPre(_, id) => send_row(send(id)?).map(|r| send_parts(r).2),
            MixerHit::AddSend(_) => send_row(track.sends.len()),
            MixerHit::Output(_) => s.output,
            MixerHit::Input(_) => s
                .input
                .filter(|_| matches!(track.kind, TrackKind::Audio | TrackKind::Instrument)),
        }
    }

    /// A point that [`Studio::mixer_hit`] answers `hit` for.
    pub fn mixer_center(&self, g: &Geometry, hit: MixerHit) -> Option<(f32, f32)> {
        let r = self.mixer_rect(g, hit)?;
        let at = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        (self.mixer_hit(g, at.0, at.1) == Some(hit)).then_some(at)
    }

    /// The value of a fader or a knob as shown: the dragged one, else the song's.
    pub fn mixer_value(&self, hit: MixerHit) -> Option<f64> {
        if let Some(d) = self.mixer.drag
            && (d.hit == hit
                || (self.track_targets(d.hit.track()).contains(&hit.track())
                    && matches!(
                        (d.hit, hit),
                        (MixerHit::Fader(_), MixerHit::Fader(_))
                            | (MixerHit::Pan(_), MixerHit::Pan(_))
                    )))
        {
            return Some(d.value);
        }
        // A channel whose lanes play shows the lanes' value at the playhead.
        let p = self.project();
        let p = xirius_session::automation::song_at(&p, self.playhead as f64);
        let t = p.tracks.get(&hit.track())?;
        match hit {
            MixerHit::Fader(_) => Some(t.volume_db),
            MixerHit::Pan(_) => Some(t.pan),
            MixerHit::SendLevel(_, s) => t.sends.iter().find(|x| x.id == s).map(send_level_db),
            _ => None,
        }
    }

    /// A press in the mixer. Faders and knobs only select here; their drag is
    /// [`Studio::drag_begin`].
    pub fn mixer_press(&mut self, g: &Geometry, hit: MixerHit) -> bool {
        self.input_map.context_delete = None;
        self.input_map.context_clipboard = None;
        let t = hit.track();
        self.mixer.master_picked = t.is_master();
        if !t.is_master() {
            self.select_row(Some(RowKind::Track(t)));
        }
        if self.refuse_frozen(hit) {
            return true;
        }
        match hit {
            MixerHit::Strip(_)
            | MixerHit::Fader(_)
            | MixerHit::Pan(_)
            | MixerHit::SendLevel(..) => true,
            MixerHit::Mute(t) => self.track_toggle("track.mute", t),
            MixerHit::Solo(t) => self.track_toggle("track.solo", t),
            MixerHit::Arm(t) => self.track_toggle("track.arm", t),
            MixerHit::Bypass(t, id) => {
                // The power mark of a plugin that is off turns it on again (ENG-008).
                let off = self
                    .project()
                    .tracks
                    .get(&t)
                    .and_then(|x| x.inserts.iter().find(|p| p.id == id))
                    .is_some_and(|p| p.deactivated);
                let command = if off {
                    "plugin.activate"
                } else {
                    "plugin.bypass"
                };
                self.run_command(command, json!({ "ids": [id.to_string()] }));
                true
            }
            MixerHit::Open(t, id) if self.health.crashed(id) => {
                let Some(anchor) = self.mixer_rect(g, hit) else {
                    return false;
                };
                self.open_popup(PopupKind::CrashedPlugin(t, id), anchor);
                true
            }
            MixerHit::Open(t, id) => self.open_plugin(t, id),
            MixerHit::SendPre(t, id) => {
                let pre = self
                    .project()
                    .tracks
                    .get(&t)
                    .and_then(|x| x.sends.iter().find(|s| s.id == id))
                    .is_some_and(|s| s.pre);
                let position = if pre { "post" } else { "pre" };
                self.run_command(
                    "send.set",
                    json!({ "id": id.to_string(), "position": position }),
                );
                true
            }
            MixerHit::AddInsert(t)
            | MixerHit::AddInstrument(t)
            | MixerHit::AddSend(t)
            | MixerHit::Output(t)
            | MixerHit::Input(t) => {
                let Some(anchor) = self.mixer_rect(g, hit) else {
                    return false;
                };
                let kind = match hit {
                    MixerHit::AddInsert(_) => PopupKind::Insert(t),
                    MixerHit::AddInstrument(_) => PopupKind::Instrument(t),
                    MixerHit::AddSend(_) => PopupKind::Send(t),
                    MixerHit::Input(_) => PopupKind::Input(t),
                    _ => PopupKind::Output(t),
                };
                self.open_popup(kind, anchor);
                true
            }
        }
    }

    /// The choices of a pick list or the output list, for the popup.
    pub(crate) fn mixer_choices(&mut self, kind: PopupKind) -> Option<Vec<Choice>> {
        let p = self.project();
        match kind {
            PopupKind::Output(t)
                if p.tracks
                    .get(&t)
                    .is_some_and(|t| t.kind == TrackKind::Master) =>
            {
                let io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
                let n = io
                    .view
                    .devices
                    .iter()
                    .find(|d| d.name == io.view.device)
                    .map_or(0, |d| d.outputs.len());
                Some(
                    (0..n)
                        .step_by(2)
                        .map(|i| Choice {
                            to: format!("{}:{}", i + 1, (i + 2).min(n)),
                            label: format!("Out {}+{}", i + 1, (i + 2).min(n)),
                        })
                        .collect(),
                )
            }
            PopupKind::Output(t) => {
                let device = self.transport.mode == xirius_session::output::OutputMode::Device;
                Some(output_choices_with(
                    &p,
                    t,
                    crate::midi_out_row::output_ports(device),
                ))
            }
            PopupKind::Input(t) => {
                let device = self.transport.mode == xirius_session::output::OutputMode::Device;
                let current = p.tracks.get(&t).and_then(|x| x.input.clone());
                // An instrument names a MIDI input port (listed, not opened).
                if p.tracks
                    .get(&t)
                    .is_some_and(|x| x.kind == TrackKind::Instrument)
                {
                    let ports = crate::input_row::midi_ports(device);
                    return Some(crate::input_row::port_choices(ports, current));
                }
                // Without a device: the test tone's two channels.
                let channels = crate::input_row::device_channels(device);
                crate::input_row::note_channels(channels);
                let names = crate::input::channel_names(channels);
                Some(input_choices(names, current))
            }
            PopupKind::Send(t) => Some(send_choices(&p, t)),
            PopupKind::Insert(_) => Some(self.insert_list()),
            PopupKind::Instrument(_) => Some(self.instrument_list()),
            _ => None,
        }
    }

    /// A choice picked in an insert or send list.
    pub(crate) fn mixer_pick(&mut self, kind: PopupKind, to: &str) {
        match kind {
            PopupKind::Insert(_) | PopupKind::Instrument(_) if to == scan::RESCAN => {
                if let Err(e) = self.start_plugin_scan() {
                    self.error = Some(e);
                }
            }
            PopupKind::Insert(_) | PopupKind::Instrument(_) if to == scan::STOP => {
                self.stop_plugin_scan()
            }
            PopupKind::Insert(t) => {
                if let Some(pick) = self.effect_pick(to) {
                    let n = self.project().tracks[&t].inserts.len();
                    self.add_effect(t, n, &pick);
                }
            }
            PopupKind::Instrument(t) => {
                self.pick_instrument(t, to);
            }
            PopupKind::InstMenu(m) => self.inst_menu_pick(m, to),
            PopupKind::Send(t) => {
                // A new bus and the send to it are one step: one undo takes
                // both away (MIX-025).
                let added = if to == NEW_BUS {
                    let name = format!("バス {}", buses(&self.project()).len() + 1);
                    self.run_command(
                        "send.add.bus",
                        json!({ "ids": [t.to_string()], "name": name, "into": "root" }),
                    )
                } else {
                    self.run_command("send.add", json!({ "ids": [t.to_string()], "to": to }))
                };
                if added {
                    self.reveal_last(t, List::Sends);
                }
            }
            _ => {}
        }
    }

    /// Scrolls a list so its last row and the ＋ row show.
    pub(crate) fn reveal_last(&mut self, t: TrackId, list: List) {
        let p = self.project();
        if let Some(track) = p.tracks.get(&t) {
            let max = list_max(track, list, Rows::of(&p));
            self.mixer.offsets.insert((t, list), max);
        }
    }

    /// Opens a plugin's own window, or shuts it when it is open (UI-025 is
    /// the window's keys; the window itself is the plugin's). A window the
    /// person closed opens again.
    pub fn open_plugin(&mut self, t: TrackId, id: PluginId) -> bool {
        if self.mixer.is_open(id) {
            let shown = self.editor_shown(id);
            self.close_editor(id);
            if shown {
                return true;
            }
        }
        if self.instruments.is_open(id) {
            self.instruments.open.retain(|w| w.plugin != id);
            return true;
        }
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let Some(plugin) = track.inserts.iter().find(|x| x.id == id) else {
            return false;
        };
        // A built-in instrument has its window here (UI-098).
        if crate::instrument::window::is_built_in(plugin) {
            self.show_instrument(t, id);
            return true;
        }
        let title = format!("{} · {}", plugin.name, track.shown_name());
        match self.open_vst3_window(plugin, title, None) {
            Ok(e) => self.mixer.editors.push(e),
            Err(e) => self.error = Some(e),
        }
        true
    }

    /// Starts dragging a fader or a knob. False when `hit`
    /// is not something that drags.
    pub fn drag_begin(&mut self, g: &Geometry, hit: Hit, y: f32) -> bool {
        match hit {
            Hit::Mixer(h @ (MixerHit::Fader(_) | MixerHit::Pan(_) | MixerHit::SendLevel(..))) => {
                // Its lane plays it (UI-063): taken, so nothing under it gets the press.
                if self.mixer_locked(h) {
                    return true;
                }
                let Some(start) = self.mixer_value(h) else {
                    return false;
                };
                let travel = match h {
                    MixerHit::Fader(_) => self
                        .mixer_rect(g, h)
                        .map_or(100.0, |r| (r.h - CAP_H).max(1.0)),
                    _ => KNOB_TRAVEL,
                };
                self.mixer.drag = Some(Drag {
                    hit: h,
                    y0: y,
                    travel,
                    start,
                    value: start,
                });
                self.auto_touch(h, Phase::Begin, Some(start));
                true
            }
            _ => false,
        }
    }

    /// The pointer moved during a drag; `fine` (Shift) moves a quarter as far.
    pub fn drag_move(&mut self, y: f32, fine: bool) -> bool {
        let Some(d) = self.mixer.drag else {
            return false;
        };
        let dy = (d.y0 - y) * if fine { 0.25 } else { 1.0 };
        let value = match d.hit {
            MixerHit::Pan(_) => {
                let v = (d.start as f32 + dy * 2.0 / d.travel).clamp(-1.0, 1.0);
                let v = if v.abs() < 0.03 { 0.0 } else { v };
                f64::from((v * 100.0).round() / 100.0)
            }
            _ => {
                let t = (db_to_t(d.start as f32) + dy / d.travel).clamp(0.0, 1.0);
                let db = t_to_db(t);
                if db <= FLOOR_DB {
                    MIN_DB
                } else {
                    f64::from(db)
                }
            }
        };
        self.drag_to(value)
    }

    /// Moves the fader or knob being dragged to `value`: the pointer's drag and a controller's
    /// knob (OPS-007) both end here.
    pub fn drag_to(&mut self, value: f64) -> bool {
        let Some(mut d) = self.mixer.drag else {
            return false;
        };
        if value == d.value {
            return false;
        }
        d.value = value;
        self.mixer.drag = Some(d);
        if self.auto_touch(d.hit, Phase::Change, Some(value)) {
            return true;
        }
        if self.transport.is_live() {
            let mut p = (*self.project()).clone();
            for t in self.auto_direct_targets(d.hit) {
                let hit = match d.hit {
                    MixerHit::Fader(_) => MixerHit::Fader(t),
                    MixerHit::Pan(_) => MixerHit::Pan(t),
                    _ => d.hit,
                };
                apply(&mut p, hit, value);
            }
            let heard = self.auto_heard(&p);
            self.transport.sync(&heard, self.song().dir());
        }
        true
    }

    /// Ends a drag: one command when the value changed.
    pub fn drag_end(&mut self) -> bool {
        let Some(d) = self.mixer.drag.take() else {
            return false;
        };
        if self.auto_touch(d.hit, Phase::End, Some(d.value)) {
            return true;
        }
        if d.value == d.start {
            // The player may have heard other values on the way.
            let p = self.project();
            self.transport.sync(&p, self.song().dir());
            return true;
        }
        let ids: Vec<_> = self
            .auto_direct_targets(d.hit)
            .iter()
            .map(ToString::to_string)
            .collect();
        match d.hit {
            MixerHit::Fader(_) => {
                self.run_command("track.volume.set", json!({ "ids": ids, "db": d.value }))
            }
            MixerHit::Pan(_) => {
                self.run_command("track.pan.set", json!({ "ids": ids, "pan": d.value }))
            }
            MixerHit::SendLevel(_, s) => {
                self.run_command("send.set", json!({ "id": s.to_string(), "level": d.value }))
            }
            _ => false,
        };
        true
    }

    /// A wheel turn over the mixer: an insert or send list moves by a row,
    /// anywhere else the strips move sideways.
    pub fn mixer_wheel(&mut self, g: &Geometry, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        if !self.mixer_shown() || !g.drawer_body.contains(x, y) {
            return false;
        }
        let p = self.project();
        for s in self.mixer_strips(g) {
            if !self.strip_holds(g, &s, x, y) {
                continue;
            }
            let list = if s.inserts.contains(x, y) {
                List::Inserts
            } else if s.sends.is_some_and(|r| r.contains(x, y)) {
                List::Sends
            } else {
                continue;
            };
            let Some(track) = p.tracks.get(&s.track) else {
                return true;
            };
            if dy != 0.0 {
                let off = self.mixer.offset(s.track, list);
                let next = if dy > 0.0 {
                    off.saturating_sub(1)
                } else {
                    (off + 1).min(list_max(track, list, s.rows))
                };
                self.mixer.offsets.insert((s.track, list), next);
            }
            return true;
        }
        let max = self.mixer_max_scroll(g);
        self.mixer.scroll = (self.mixer.scroll - (dx + dy)).clamp(0.0, max);
        true
    }

    /// The song changed: the mixer follows.
    pub(crate) fn mixer_follow(&mut self) {
        let p = self.project();
        self.mixer.follow(&p);
    }
}

#[cfg(test)]
mod editor_tests;
#[cfg(test)]
mod master_tests;
#[cfg(test)]
mod tests;
