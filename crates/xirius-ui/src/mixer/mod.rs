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
pub(crate) mod choose;
pub(crate) mod dock;
pub(crate) mod editors;
pub mod flow;
pub(crate) mod group;
pub mod keys;
pub mod knob;
pub mod meters;
pub(crate) mod paint;
pub mod scan;

pub use paint::paint;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::json;
use xirius_core::controls::ControlId;
use xirius_core::id::{NodeId, PluginId, SendId, TrackId};
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
/// A strip widened by the button by its name (MIX-045).
pub const WIDE_W: f32 = 252.0;
/// What the screen reader calls the buttons by a strip's name (MIX-045).
pub const EDITOR_LABEL: &str = "チャンネルエディターを開く";
pub const WIDEN_LABEL: &str = "広げる";
/// The strip's area headings, and what their ＋ adds: the same words on the strip, in its
/// menus and in what the screen reader says (MIX-044).
pub const INSTRUMENT_HEAD: &str = "インストゥルメント";
pub const INSERT_HEAD: &str = "インサート";
pub const ADD_EFFECT_LABEL: &str = "エフェクトを追加";
pub const ADD_INSTRUMENT_LABEL: &str = "インストゥルメントを追加";
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
/// The least the fader's part of a strip keeps under the flow: the pan and three buttons.
const MIN_ZONE: f32 = KNOB + BTN_GAP + 3.0 * (BTN + BTN_GAP);
/// The space between the channels and the master (the mock's `.mixer` gap).
pub(crate) const MASTER_GAP: f32 = 6.0;
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
    /// The ＋ of the instrument area of a track that has one: another layer (MIX-044).
    AddLayer(TrackId),
    /// A splitter's chevron in the strip's flow: its branches shut or open (MIX-044).
    Fold(TrackId, NodeId),
    SendLevel(TrackId, SendId),
    /// A key send's row (MIX-048): the receiving plug-in's window.
    KeySend(TrackId, SendId),
    /// The bar of a key send's level (MIX-048).
    KeyLevel(TrackId, SendId),
    /// The glyph that sends from before or after the fader.
    SendPre(TrackId, SendId),
    AddSend(TrackId),
    Output(TrackId),
    /// The input row of an audio track: the device it records (REC-017).
    Input(TrackId),
    /// A macro control of the track (MIX-039): a controller's knob moves it as it moves a fader.
    Control(TrackId, ControlId),
    /// The M of a row of the flow: the branch it heads or its layer mutes (MIX-045).
    RowMute(TrackId, NodeId),
    /// The bar of the level row under the picked row (MIX-045).
    Level(TrackId, NodeId),
    /// The ＋ on a row's bottom edge: a plug-in after it (MIX-045).
    After(TrackId, flow::Pick),
    /// The button by the name that widens the strip or narrows it again (MIX-045).
    Widen(TrackId),
    /// The button by the name that opens the channel editor (MIX-042).
    Editor(TrackId),
    /// An area's heading in the flow: the insert heading drags the inserts to other channels
    /// and takes Ctrl+C / Ctrl+V for them (MIX-045).
    Heading(TrackId, flow::Area),
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
            | MixerHit::AddLayer(t)
            | MixerHit::Fold(t, _)
            | MixerHit::SendLevel(t, _)
            | MixerHit::KeySend(t, _)
            | MixerHit::KeyLevel(t, _)
            | MixerHit::SendPre(t, _)
            | MixerHit::AddSend(t)
            | MixerHit::Output(t)
            | MixerHit::Input(t)
            | MixerHit::Control(t, _)
            | MixerHit::RowMute(t, _)
            | MixerHit::Level(t, _)
            | MixerHit::After(t, _)
            | MixerHit::Widen(t)
            | MixerHit::Editor(t)
            | MixerHit::Heading(t, _) => t,
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
    /// The flow (MIX-044): the instrument and insert areas, as high on every strip.
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
/// to the faders. `flow` is the height the fullest strip's flow asks for (MIX-044); the strip
/// gives it what the fader leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rows {
    pub inserts: usize,
    pub sends: usize,
    pub flow: u32,
}

impl Rows {
    pub fn of(p: &Project) -> Rows {
        Self::with(
            p,
            flow::Look {
                folds: &flow::Folds::default(),
                wide: &HashSet::new(),
                at: Some(false),
                chosen: None,
                coarse: false,
            },
        )
    }

    /// As the strips show their flows (the splitters shut, the row picked, touch).
    pub fn with(p: &Project, look: flow::Look) -> Rows {
        let most = |n: fn(&Track) -> usize| p.tracks.values().map(|t| n(t) + 1).max().unwrap_or(1);
        let flow = p
            .tracks
            .values()
            .map(|t| look.height(t).ceil() as u32)
            .max()
            .unwrap_or(ROW_H as u32);
        Rows {
            inserts: most(|t| t.inserts.len() + crate::instrument::gap(t)).clamp(1, INSERT_ROWS),
            sends: p
                .tracks
                .values()
                .map(|t| {
                    if t.id.is_master() {
                        1
                    } else {
                        t.sends.len() + keys::key_count(p, t.id) + 1
                    }
                })
                .max()
                .unwrap_or(1)
                .clamp(1, SEND_ROWS),
            flow,
        }
    }

    fn of_list(self, list: List) -> usize {
        match list {
            List::Inserts => self.inserts,
            List::Sends => self.sends,
            List::Flow => 1,
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
        // From the bottom: the output, the sends; the faders take what is left,
        // so they line up across strips of every kind.
        let output = Rect::new(body_x, inner.bottom() - ROW_H, body_w, ROW_H);
        let sends = Rect::new(
            body_x,
            output.y - GAP - list_h(rows.sends),
            body_w,
            list_h(rows.sends),
        );
        // The flow takes what it asks for, up to what leaves the fader its knob and buttons;
        // the same on the master, so the faders line up.
        let room = (sends.y - GAP - y - GAP - MIN_ZONE).max(ROW_H);
        let inserts = Rect::new(inner.x, y, inner.w, (rows.flow as f32).min(room));
        y += inserts.h + GAP;
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
        let body = self.output.unwrap_or(self.name);
        Rect::new(body.x, y + (KNOB - ROW_H) / 2.0, body.w, ROW_H)
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
    /// A track header's insert rows.
    Inserts,
    Sends,
    /// A strip's flow (MIX-044), by rows of [`ROW_H`].
    Flow,
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
    /// Alt was held at the press: the dragged track moves alone (MIX-050).
    pub alone: bool,
}

/// The bar of a level row held (MIX-045): the branch or layer it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelDrag {
    pub track: TrackId,
    pub node: NodeId,
    pub layer: bool,
    start: f64,
    /// The value shown and heard now.
    pub value: f64,
    x0: f32,
    t0: f32,
    /// The bar's width.
    w: f32,
}

impl LevelDrag {
    /// A row's level taken at `x` on its bar `bar` (the strip's row, and the channel editor's
    /// heads of MIX-042).
    pub(crate) fn taken(t: TrackId, target: flow::Target, bar: Rect, x: f32) -> LevelDrag {
        let w = bar.w.max(1.0);
        LevelDrag {
            track: t,
            node: target.id,
            layer: target.layer,
            start: target.db,
            value: target.db,
            x0: x,
            t0: ((x - bar.x) / w).clamp(0.0, 1.0),
            w,
        }
    }
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
    /// A channel's insert heading held, its inserts on their way to another channel (MIX-045).
    pub insert_drag: Option<crate::insert_drag::Drag>,
    /// The channel whose insert heading has the keys: Ctrl+C and Ctrl+V copy and paste its
    /// inserts (MIX-045).
    pub heading: Option<TrackId>,
    /// The master, when it is the strip pressed last (a selected row never is the master).
    pub master_picked: bool,
    pub recent: Vec<crate::instrument::Pick>,
    pub preset_panel: Option<crate::fx_preset::Panel>,
    pub browser_fx_focus: bool,
    /// A file supplied by a caller in place of the system's preset picker.
    pub preset_file: Option<std::path::PathBuf>,
    offsets: HashMap<(TrackId, List), usize>,
    /// Which splitters the strips' flows show open (MIX-044).
    pub folds: flow::Folds,
    /// The row picked on a strip (MIX-045).
    pub chosen: Option<(TrackId, flow::Pick)>,
    /// The strips widened by the button by their names (MIX-045).
    pub wide: HashSet<TrackId>,
    /// The last press was a touch: the flows' rows are 32 high (MIX-045).
    pub coarse: bool,
    /// What the press held now was, for [`Mixer::coarse`] once it is let go.
    pub coarse_next: Option<bool>,
    pub level_drag: Option<LevelDrag>,
    /// The bar of a key send held (MIX-048).
    pub key_drag: Option<keys::KeyDrag>,
    /// The key send whose bar ←→ move: the one pressed last (MIX-048).
    pub key_focus: Option<(TrackId, SendId)>,
    /// The gain reduction of each receiving plug-in a strip shows, in dB (MIX-048).
    pub gr: HashMap<PluginId, f32>,
    /// The probes the mixer watches for [`Mixer::gr`].
    pub(crate) gr_watched: Vec<PluginId>,
    /// The node the open plug-in list puts its pick after (the ＋ on a row's edge).
    pub insert_after: Option<(TrackId, flow::Pick)>,
    /// The channel editor's ＋ the insert list was opened from, when nothing is before it in
    /// its chain (MIX-042): the new effect is moved there.
    pub insert_into: Option<(TrackId, crate::channel::flow::Chain, usize)>,
    /// The plugin just put in whose window opens once it is loaded (PLG-050).
    pub(crate) open_later: Option<crate::loading::OpenLater>,
    #[cfg(test)]
    pub(crate) fake_load: Option<crate::loading::FakeLoad>,
    pub(crate) editors: Vec<OpenEditor>,
    /// The plugins found by the last scan; read when a list first needs it.
    pub catalog: Option<Arc<Catalog>>,
    pub scan: scan::PluginScan,
    /// How long the last paint took, in µs (for the measurement of UI-024).
    pub paint_us: Cell<u64>,
    /// The channel a double click on a track's head brought up, and when it did (ms).
    flash: Option<(TrackId, u64)>,
    /// The band pressed last while the strips are folded: its strip stays open (MIX-046).
    pub lead: Option<TrackId>,
    /// The grip between the strips and the face is held: the face's width and the pointer's
    /// x when it was taken (MIX-046).
    pub grip_drag: Option<(f32, f32)>,
    /// The grip was pressed last: ←→ move it (MIX-046).
    pub grip_focus: bool,
    /// The pointer is over the grip.
    pub grip_hover: bool,
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
    pub(crate) fn set_flow_offset(&mut self, t: TrackId, offset: usize) {
        self.offsets.insert((t, List::Flow), offset);
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

    /// How the strips show their flows.
    pub fn look(&self) -> flow::Look<'_> {
        flow::Look {
            folds: &self.folds,
            wide: &self.wide,
            at: None,
            chosen: self.chosen,
            coarse: self.coarse,
        }
    }

    /// The song changed: windows of plugins that are gone shut, lists keep
    /// their ＋ row in reach.
    fn follow(&mut self, p: &Project) {
        if let Some((t, pick)) = self.chosen
            && !p.tracks.get(&t).is_some_and(|t| match pick {
                flow::Pick::Plugin(id) => t.inserts.iter().any(|x| x.id == id),
                flow::Pick::Split(id) => flow::has_split(t, id),
            })
        {
            self.chosen = None;
        }
        self.wide.retain(|t| p.tracks.contains_key(t));
        if self.level_drag.is_some_and(|d| {
            p.track(d.track)
                .ok()
                .and_then(|t| t.rack.as_ref())
                .and_then(|r| r.levels(d.node))
                .is_none()
        }) {
            self.level_drag = None;
        }
        // only the lists' rows are read here, and they are the same at any width
        let rows = Rows::with(p, self.look());
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
            *off = (*off).min(list_max(p, track, *list, rows));
            true
        });
        // what a person opened or shut stays for a splitter that is gone: an undo brings the
        // same splitter back, and a song opened clears it all
        if let Some(d) = self.drag
            && !p.tracks.contains_key(&d.hit.track())
        {
            self.drag = None;
        }
    }
}

/// Where the insert list's Splitter goes: after the row it was opened from, into the branch
/// or layer, or at the end of the track's effects.
pub(crate) fn split_at(
    after: Option<(TrackId, flow::Pick)>,
    into: Option<(TrackId, crate::channel::flow::Chain, usize)>,
) -> String {
    use crate::channel::flow::Chain;
    match (after, into) {
        (Some((_, flow::Pick::Plugin(id))), _) => format!("after:{id}"),
        (Some((_, flow::Pick::Split(id))), _) => format!("after:{id}"),
        (None, Some((_, Chain::Branch(n), i))) => format!("branch:{n}:{i}"),
        (None, Some((_, Chain::Layer(n), i))) => format!("layer:{n}:{i}"),
        (None, _) => "track".to_string(),
    }
}

/// How far a list can scroll: its rows and the ＋ row, less what fits.
fn list_max(p: &Project, track: &Track, list: List, rows: Rows) -> usize {
    let n = match list {
        List::Inserts => track.inserts.len() + crate::instrument::gap(track),
        List::Sends => track.sends.len() + keys::key_count(p, track.id),
        // The layout holds a flow's scroll to what is hidden; here only a bound.
        List::Flow => return flow::lines(track, &flow::Folds::default(), true).len(),
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
        MixerHit::Control(_, c) => {
            t.controls.get_mut(c).value = value;
            t.apply_control_at(c, value);
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

    /// How wide `t`'s strip is: [`WIDE_W`] when its button widened it (MIX-045). While the
    /// strips are folded (MIX-046), the one in front is [`WIDE_W`] and the others bands.
    pub(crate) fn width_of(&self, g: &Geometry, t: TrackId) -> f32 {
        if self.mixer_folded(g) {
            if self.mixer_lead() == Some(t) {
                WIDE_W
            } else {
                dock::BAND_W
            }
        } else if self.mixer.wide.contains(&t) {
            WIDE_W
        } else {
            self.strip_width()
        }
    }

    /// The strips at [`WIDE_W`] now: the widened ones, or the one in front while folded.
    fn wide_now(&self, g: &Geometry) -> HashSet<TrackId> {
        if self.mixer_folded(g) {
            self.mixer_lead().into_iter().collect()
        } else {
            self.mixer.wide.clone()
        }
    }

    /// Where `t`'s strip starts in the scrolled row of channels (0 at the first's left).
    pub(crate) fn strip_offset(&self, g: &Geometry, t: TrackId) -> Option<f32> {
        let mut x = 0.0;
        for id in strip_order(&self.project())
            .into_iter()
            .filter(|id| !id.is_master())
        {
            if id == t {
                return Some(x);
            }
            x += self.width_of(g, id) + STRIP_GAP;
        }
        None
    }

    /// The scroll a bank of channels further on (or back) starts at: always a strip's left
    /// edge, strips of every width counted as they are (MIX-045). On, the first strip not
    /// wholly in view comes to the left; back, as many whole strips as fit end where the
    /// first one began.
    pub(crate) fn mixer_page(&self, g: &Geometry, next: bool) -> f32 {
        let view = self.mixer_view(g).w;
        let mut strips = Vec::new();
        let mut x = 0.0;
        for id in strip_order(&self.project())
            .into_iter()
            .filter(|id| !id.is_master())
        {
            let w = self.width_of(g, id);
            strips.push((x, x + w));
            x += w + STRIP_GAP;
        }
        let before = self.mixer.scroll;
        let to = if next {
            strips
                .iter()
                .find(|(l, r)| *l > before + 0.5 && *r > before + view)
                .map_or(before, |(l, _)| *l)
        } else {
            let earlier: Vec<f32> = (strips.iter().map(|(l, _)| *l))
                .filter(|l| *l < before - 0.5)
                .collect();
            earlier
                .iter()
                .copied()
                .find(|l| *l >= before - view)
                .or(earlier.last().copied())
                .unwrap_or(0.0)
        };
        to.clamp(0.0, self.mixer_max_scroll(g))
    }

    /// The part of the body the channels scroll in: all of it left of the master, which
    /// stays at the right edge (the master is not a channel that scrolls away).
    pub fn mixer_view(&self, g: &Geometry) -> Rect {
        let body = self.mixer_body(g);
        let master = self.project().tracks.contains_key(&TrackId::MASTER);
        let w = if master {
            body.w - STRIP_GAP - self.width_of(g, TrackId::MASTER) - MASTER_GAP
        } else {
            body.w
        };
        Rect::new(body.x, body.y, w.max(0.0), body.h)
    }

    /// The strips on screen, left to right: the channels that show in the view, then the
    /// master. A channel at the view's right edge may reach under the master, so the
    /// parts of a channel count only where [`Studio::mixer_view`] holds the point.
    pub fn mixer_strips(&self, g: &Geometry) -> Vec<StripLayout> {
        self.mixer_row(g).0
    }

    /// The strips on screen and, while they are folded (MIX-046), the bands.
    pub(crate) fn mixer_row(&self, g: &Geometry) -> (Vec<StripLayout>, Vec<(TrackId, Rect)>) {
        let p = self.project();
        let body = self.mixer_body(g);
        let view = self.mixer_view(g);
        let folded = self.mixer_folded(g);
        let lead = self.mixer_lead();
        let band = |t: TrackId| folded && lead != Some(t);
        let mut bands = Vec::new();
        let (top, h) = (
            body.y + STRIP_GAP,
            (body.h - STRIP_GAP * 2.0 - BAR_H - BAR_GAP).max(0.0),
        );
        // the flows' height as each strip shows its own, at the width it has now
        let wide = self.wide_now(g);
        let rows = Rows::with(
            &p,
            flow::Look {
                wide: &wide,
                ..self.mixer.look()
            },
        );
        let mut left = view.x + STRIP_GAP - self.mixer_scroll(g);
        let mut out: Vec<StripLayout> = strip_order(&p)
            .into_iter()
            .filter(|id| !id.is_master())
            .filter_map(|id| {
                let width = self.width_of(g, id);
                let x = left;
                left += width + STRIP_GAP;
                if x + width <= view.x || x >= view.right() {
                    return None;
                }
                let kind = p.tracks.get(&id)?.kind;
                if band(id) {
                    bands.push((id, Rect::new(x, top, width, h)));
                    return None;
                }
                Some(StripLayout::sized(id, kind, x, top, h, rows, width))
            })
            .collect();
        if let Some(master) = p.tracks.get(&TrackId::MASTER) {
            let width = self.width_of(g, master.id);
            let x = body.right() - STRIP_GAP - width;
            if band(master.id) {
                bands.push((master.id, Rect::new(x, top, width, h)));
                return (out, bands);
            }
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
        (out, bands)
    }

    /// How far the channels are scrolled, as far as they go. While they are folded
    /// (MIX-046), the strip in front stays in sight.
    pub(crate) fn mixer_scroll(&self, g: &Geometry) -> f32 {
        let s = self.mixer.scroll.min(self.mixer_max_scroll(g)).max(0.0);
        if !self.mixer_folded(g) {
            return s;
        }
        let Some(off) = self.mixer_lead().and_then(|t| self.strip_offset(g, t)) else {
            return s;
        };
        let lo = (off + WIDE_W + 2.0 * STRIP_GAP - self.mixer_view(g).w).max(0.0);
        s.max(lo).min(off)
    }

    /// Whether the point is on the strip: a channel only where the view shows it.
    pub(crate) fn strip_holds(&self, g: &Geometry, s: &StripLayout, x: f32, y: f32) -> bool {
        s.rect.contains(x, y) && (s.track.is_master() || self.mixer_view(g).contains(x, y))
    }

    /// How far the channels scroll: what does not fit left of the master.
    pub fn mixer_max_scroll(&self, g: &Geometry) -> f32 {
        let all: f32 = strip_order(&self.project())
            .into_iter()
            .filter(|t| !t.is_master())
            .map(|t| self.width_of(g, t) + STRIP_GAP)
            .sum();
        (STRIP_GAP + all - self.mixer_view(g).w).max(0.0)
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
            self.mixer_body(g).bottom() - STRIP_GAP - BAR_H,
            (view.w - STRIP_GAP * 2.0).max(0.0),
            BAR_H,
        );
        let len = (groove.w * view.w / (view.w + max)).clamp(BAR_MIN.min(groove.w), groove.w);
        let x = groove.x + (groove.w - len) * (self.mixer_scroll(g) / max).clamp(0.0, 1.0);
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
        if !self.mixer_shown() || !self.mixer_body(g).contains(x, y) {
            return None;
        }
        if let Some((t, _)) = self
            .mixer_bands(g)
            .into_iter()
            .find(|(t, r)| r.contains(x, y) && (t.is_master() || self.mixer_view(g).contains(x, y)))
        {
            return Some(MixerHit::Strip(t));
        }
        let p = self.project();
        let s = self
            .mixer_strips(g)
            .into_iter()
            .find(|s| self.strip_holds(g, s, x, y))?;
        let t = s.track;
        let track = p.tracks.get(&t)?;
        let on = |r: Option<Rect>| r.is_some_and(|r| r.contains(x, y));
        let (editor, widen) = paint::name_tools(&s);
        let hit = if editor.contains(x, y) {
            MixerHit::Editor(t)
        } else if widen.contains(x, y) {
            MixerHit::Widen(t)
        } else if on(s.pan) {
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
            flow::hit(self, track, &self.strip_flow(&s, track), x, y)
        } else if on(s.sends) {
            let list = s.sends.expect("on a send list");
            let Some(i) = row_at(list, y) else {
                return Some(MixerHit::Strip(t));
            };
            let k = self.mixer.offset(t, List::Sends) + i;
            let row = list_row(list, i);
            let (_, knob, pp) = send_parts(row);
            let keyed = keys::key_sends(&p, t);
            let n = track.sends.len();
            match (
                track.sends.get(k),
                k.checked_sub(n).and_then(|j| keyed.get(j)),
            ) {
                (Some(send), _) if knob.contains(x, y) => MixerHit::SendLevel(t, send.id),
                (Some(send), _) if pp.contains(x, y) => MixerHit::SendPre(t, send.id),
                (Some(_), _) => MixerHit::Strip(t),
                (None, Some(key)) if keys::key_parts(row).3.contains(x, y) => {
                    MixerHit::KeyLevel(t, key.id)
                }
                (None, Some(key)) => MixerHit::KeySend(t, key.id),
                (None, None) if k == n + keyed.len() => MixerHit::AddSend(t),
                (None, None) => MixerHit::Strip(t),
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
        if let MixerHit::Strip(_) = hit
            && let Some((_, r)) = self.mixer_bands(g).into_iter().find(|(x, _)| *x == t)
        {
            return Some(r);
        }
        let s = self.mixer_strips(g).into_iter().find(|s| s.track == t)?;
        let p = self.project();
        let track = p.tracks.get(&t)?;
        let flow = self.strip_flow(&s, track);
        // A row of the flow counts where its middle is in sight.
        let seen = |r: Rect| s.inserts.contains(r.x + r.w / 2.0, r.y + r.h / 2.0);
        let plugin_row = |id: PluginId| flow.plugin(id).filter(|(_, r)| seen(*r));
        let send_row = |k: usize| {
            let i = k.checked_sub(self.mixer.offset(t, List::Sends))?;
            let list = s.sends?;
            (i < s.rows.sends).then(|| list_row(list, i))
        };
        let send = |id: SendId| track.sends.iter().position(|x| x.id == id);
        let keyed = keys::key_sends(&p, t);
        let key = |id: SendId| {
            let j = keyed.iter().position(|x| x.id == id)?;
            send_row(track.sends.len() + j)
        };
        match hit {
            MixerHit::Strip(_) => Some(s.name),
            MixerHit::Fader(_) => Some(s.fader),
            MixerHit::Pan(_) => s.pan,
            MixerHit::Mute(_) => s.mute,
            MixerHit::Solo(_) => s.solo,
            MixerHit::Arm(_) => s.arm,
            MixerHit::Bypass(_, id) => plugin_row(id).map(|(l, r)| flow::dot_part(*r, l)),
            MixerHit::Open(_, id) if self.health.crashed(id) => plugin_row(id).map(|(_, r)| *r),
            MixerHit::Open(_, id) => plugin_row(id).map(|(l, r)| {
                let dot = flow::dot_part(*r, l);
                Rect::new(dot.right(), r.y, r.right() - dot.right(), r.h)
            }),
            MixerHit::AddInsert(_) => flow.add(flow::Area::Insert).filter(|r| seen(*r)),
            MixerHit::AddInstrument(_) if flow.has_instrument() => None,
            MixerHit::AddLayer(_) if !flow.has_instrument() => None,
            MixerHit::AddInstrument(_) | MixerHit::AddLayer(_) => {
                flow.add(flow::Area::Instrument).filter(|r| seen(*r))
            }
            MixerHit::Heading(_, area) => flow.head(area).filter(|r| seen(*r)),
            MixerHit::Fold(_, id) => flow
                .split(id)
                .filter(|(_, r)| seen(*r))
                .map(|(l, r)| flow::fold_part(*r, l)),
            MixerHit::SendLevel(_, id) => send_row(send(id)?).map(|r| send_parts(r).1),
            MixerHit::SendPre(_, id) => send_row(send(id)?).map(|r| send_parts(r).2),
            MixerHit::KeySend(_, id) => key(id),
            MixerHit::KeyLevel(_, id) => key(id).map(|r| keys::key_parts(r).3),
            MixerHit::AddSend(_) => send_row(track.sends.len() + keyed.len()),
            MixerHit::Output(_) => s.output,
            MixerHit::Input(_) => s
                .input
                .filter(|_| matches!(track.kind, TrackKind::Audio | TrackKind::Instrument)),
            // a macro sits in the channel editor, not on the strip
            MixerHit::Control(..) => None,
            MixerHit::RowMute(_, n) => flow
                .rows
                .iter()
                .filter(|(l, r)| seen(*r) && l.target.is_some_and(|x| x.id == n))
                .find_map(|(l, r)| match l.kind {
                    flow::Kind::Level => Some(flow::sub_parts(*r).2),
                    _ if self.mixer.chosen.is_none_or(|c| Some(c.1) != l.pick()) => {
                        Some(flow::mute_part(*r, l))
                    }
                    _ => None,
                }),
            MixerHit::Level(_, n) => flow
                .level()
                .filter(|(l, r)| seen(*r) && l.target.is_some_and(|x| x.id == n))
                .map(|(_, r)| flow::sub_parts(*r).0),
            MixerHit::After(_, pick) => flow
                .picked(pick)
                .filter(|(_, r)| seen(*r))
                .and_then(|_| flow.after(pick)),
            MixerHit::Widen(_) => Some(paint::name_tools(&s).1),
            MixerHit::Editor(_) => Some(paint::name_tools(&s).0),
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
        if let Some(d) = self.mixer.drag {
            // Every selected track moves from where it was (MIX-050), but one its lane holds
            // (UI-063), which the release leaves too.
            if self.drag_relative()
                && group::same_on(d.hit, hit.track()) == hit
                && self.drag_tracks(d.hit).contains(&hit.track())
                && (hit == d.hit || !self.mixer_locked(hit))
            {
                let now = if hit == d.hit {
                    Some(d.start)
                } else {
                    self.mixer_rest(hit)
                };
                return now.map(|now| group::moved(hit, now, d.by()));
            }
            if d.hit == hit {
                return Some(d.value);
            }
        }
        self.mixer_rest(hit)
    }

    /// The value of a fader or a knob when nothing drags it.
    pub(crate) fn mixer_rest(&self, hit: MixerHit) -> Option<f64> {
        // A channel whose lanes play shows the lanes' value at the playhead.
        // Only this channel's lanes are read: a screen of many faders does not take the whole
        // song's for each of them.
        let p = self.project();
        let t = xirius_session::automation::track_at(&p, hit.track(), self.playhead as f64)?;
        match hit {
            MixerHit::Fader(_) => Some(t.volume_db),
            MixerHit::Pan(_) => Some(t.pan),
            MixerHit::SendLevel(_, s) => t.sends.iter().find(|x| x.id == s).map(send_level_db),
            MixerHit::Control(_, c) => Some(t.controls.get(c).value),
            _ => None,
        }
    }

    /// A press in the mixer. Faders and knobs do nothing more than choose here; their drag is
    /// [`Studio::drag_begin`].
    pub fn mixer_press(&mut self, g: &Geometry, hit: MixerHit) -> bool {
        self.input_map.context_delete = None;
        self.input_map.context_clipboard = None;
        let t = hit.track();
        self.mixer.master_picked = t.is_master();
        if !matches!(hit, MixerHit::KeyLevel(..)) {
            self.mixer.key_focus = None;
        }
        self.mixer.heading = match hit {
            MixerHit::Heading(t, flow::Area::Insert) => Some(t),
            _ => None,
        };
        if !t.is_master() {
            // The name and the ground choose as a head does (UI-340); any other part of a track
            // outside the selection acts on that track and leaves the selection as it is
            // (MIX-050).
            if matches!(hit, MixerHit::Strip(_) | MixerHit::Heading(..)) {
                self.pick_row(RowKind::Track(t), self.arrange.mods);
            } else if self.is_picked(RowKind::Track(t)) {
                self.select_row(Some(RowKind::Track(t)));
            }
        }
        if self.refuse_frozen(hit) {
            return true;
        }
        if self.mixer_folded(g) {
            self.mixer.lead = Some(t);
        }
        match hit {
            MixerHit::Strip(_)
            | MixerHit::Heading(..)
            | MixerHit::Fader(_)
            | MixerHit::Pan(_)
            | MixerHit::SendLevel(..) => true,
            MixerHit::KeyLevel(t, id) => {
                self.mixer.key_focus = Some((t, id));
                true
            }
            MixerHit::KeySend(t, id) => {
                let Some(k) = keys::key_send(&self.project(), t, id) else {
                    return false;
                };
                self.open_plugin(k.owner, k.plugin)
            }
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
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
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
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "send.set",
                    json!({ "id": id.to_string(), "position": position }),
                );
                true
            }
            MixerHit::Fold(t, id) => {
                self.fold_split(g, t, id);
                self.mixer.chosen = Some((t, flow::Pick::Split(id)));
                true
            }
            MixerHit::RowMute(t, n) => self.row_mute(t, n),
            MixerHit::Level(..) => true,
            MixerHit::After(t, pick) => {
                let Some(anchor) = self.mixer_rect(g, hit) else {
                    return false;
                };
                self.insert_after(t, pick, anchor);
                true
            }
            MixerHit::Widen(t) => {
                if !self.mixer.wide.remove(&t) {
                    self.mixer.wide.insert(t);
                }
                self.mixer.scroll = self.mixer.scroll.min(self.mixer_max_scroll(g));
                true
            }
            MixerHit::Editor(t) => {
                self.open_channel(t);
                true
            }
            MixerHit::AddInsert(t)
            | MixerHit::AddInstrument(t)
            | MixerHit::AddLayer(t)
            | MixerHit::AddSend(t)
            | MixerHit::Output(t)
            | MixerHit::Input(t) => {
                let Some(anchor) = self.mixer_rect(g, hit) else {
                    return false;
                };
                self.mixer.insert_after = None;
                self.mixer.insert_into = None;
                let kind = match hit {
                    MixerHit::AddInsert(_) => PopupKind::Insert(t),
                    MixerHit::AddInstrument(_) => PopupKind::Instrument(t),
                    MixerHit::AddLayer(_) => PopupKind::Layer(t),
                    MixerHit::AddSend(_) => PopupKind::Send(t),
                    MixerHit::Input(_) => PopupKind::Input(t),
                    _ => PopupKind::Output(t),
                };
                self.open_popup(kind, anchor);
                true
            }
            MixerHit::Control(..) => false,
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
                // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
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
            _ => None,
        }
    }

    /// A choice picked in an insert or send list.
    pub(crate) fn mixer_pick(&mut self, kind: PopupKind, to: &str) {
        match kind {
            PopupKind::AraPlugin => self.ara_picked(to),
            PopupKind::Insert(_) | PopupKind::Instrument(_) | PopupKind::Layer(_)
                if to == scan::RESCAN =>
            {
                if let Err(e) = self.start_plugin_scan() {
                    self.error = Some(e);
                }
            }
            PopupKind::Insert(_) | PopupKind::Instrument(_) | PopupKind::Layer(_)
                if to == scan::STOP =>
            {
                self.stop_plugin_scan()
            }
            PopupKind::Insert(t) => {
                let after = self.mixer.insert_after.take().filter(|(x, _)| *x == t);
                let into = self.mixer.insert_into.take().filter(|(x, ..)| *x == t);
                if to == scan::SPLITTER {
                    let at = split_at(after, into);
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.arrange_run(
                        "rack.split.add",
                        json!({ "track": t.to_string(), "at": at }),
                    );
                } else if let Some(pick) = self.effect_pick(to) {
                    match (after, into) {
                        (Some((_, after)), _) => {
                            self.add_effect_after(t, after, &pick);
                        }
                        (None, Some((_, chain, index))) => {
                            self.add_effect_into(t, chain, index, &pick);
                        }
                        (None, None) => {
                            let n = self.project().tracks[&t].inserts.len();
                            self.add_effect(t, n, &pick);
                        }
                    }
                }
            }
            PopupKind::Instrument(t) => {
                self.pick_instrument(t, to);
            }
            PopupKind::Layer(t) => {
                self.add_layer(t, to);
            }
            PopupKind::InstMenu(m) => self.inst_menu_pick(m, to),
            PopupKind::Send(t) if to.starts_with(keys::PICK) => {
                let Ok(plugin) = to[keys::PICK.len()..].parse::<xirius_core::id::PluginId>() else {
                    return;
                };
                let args = json!({ "id": plugin.to_string(), "from": t.to_string() });
                if keys::keys(&self.project(), t, plugin) {
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.run_command("plugin.sidechain.remove", args);
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                } else if self.run_command("plugin.sidechain.add", args) {
                    self.reveal_last(t, List::Sends);
                }
            }
            PopupKind::Send(t) => {
                // A new bus and the send to it are one step: one undo takes
                // both away (MIX-025).
                let added = if to == NEW_BUS {
                    let name = format!("バス {}", buses(&self.project()).len() + 1);
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.run_command(
                        "send.add.bus",
                        json!({ "ids": [t.to_string()], "name": name, "into": "root" }),
                    )
                } else {
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
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
            // the lists' rows are the same at any width
            let max = list_max(&p, track, list, Rows::with(&p, self.mixer.look()));
            self.mixer.offsets.insert((t, list), max);
            // A new insert is the last row of the strip's flow too.
            if list == List::Inserts {
                let max = list_max(&p, track, List::Flow, Rows::of(&p));
                self.mixer.offsets.insert((t, List::Flow), max);
            }
        }
    }

    /// `track`'s flow on strip `s`, scrolled as far as its list was moved and no further
    /// than what is hidden (MIX-044).
    pub fn strip_flow(&self, s: &StripLayout, track: &Track) -> flow::Flow {
        let look = self.mixer.look().at(s.rect.w >= WIDE_W);
        let lines = look.lines(track);
        let h = flow::Flow::height(&lines, look.coarse);
        let scroll = (self.mixer.offset(track.id, List::Flow) as f32 * ROW_H)
            .min((h - s.inserts.h).max(0.0));
        flow::Flow::lay(&lines, s.inserts, scroll, look.coarse)
    }

    /// The row of plug-in `id` on `t`'s strip from its indent on, where its marks are.
    pub(crate) fn strip_item(&self, g: &Geometry, t: TrackId, id: PluginId) -> Option<Rect> {
        let s = self.mixer_strips(g).into_iter().find(|s| s.track == t)?;
        let p = self.project();
        let flow = self.strip_flow(&s, p.tracks.get(&t)?);
        flow.plugin(id).map(|(l, r)| flow::item(*r, l))
    }

    /// Scrolls `t`'s flow so plug-in `id`'s row is in it, from above or from below.
    pub(crate) fn reveal_in_flow(&mut self, g: &Geometry, t: TrackId, id: PluginId) {
        self.reveal_pick(g, t, flow::Pick::Plugin(id));
    }

    /// Scrolls `t`'s flow so a picked row (and its level row) is in it.
    pub(crate) fn reveal_pick(&mut self, g: &Geometry, t: TrackId, pick: flow::Pick) {
        let Some(s) = self.mixer_strips(g).into_iter().find(|s| s.track == t) else {
            return;
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return;
        };
        let flow = self.mixer.look().at(s.rect.w >= WIDE_W).lay(
            track,
            Rect::new(0.0, 0.0, s.inserts.w, 0.0),
            0.0,
        );
        let Some((l, r)) = flow.picked(pick) else {
            return;
        };
        let mut r = *r;
        if let Some((_, sub)) = flow
            .level()
            .filter(|(sl, _)| sl.target.is_some() && sl.target == l.target)
        {
            r.h = sub.bottom() - r.y;
        }
        let max = self.flow_max(&s, track);
        let off = self.mixer.offset(t, List::Flow).min(max);
        let top = off as f32 * ROW_H;
        let next = if r.y < top {
            (r.y / ROW_H).floor() as usize
        } else if r.bottom() > top + s.inserts.h {
            ((r.bottom() - s.inserts.h) / ROW_H).ceil() as usize
        } else {
            off
        };
        self.mixer.set_flow_offset(t, next.min(max));
    }

    /// How many rows of [`ROW_H`] a strip's flow can move up: what it hides, rounded up.
    pub(crate) fn flow_max(&self, s: &StripLayout, track: &Track) -> usize {
        let h = self.mixer.look().at(s.rect.w >= WIDE_W).height(track);
        ((h - s.inserts.h).max(0.0) / ROW_H).ceil() as usize
    }

    /// Opens splitter `id` on `t`'s strip when it is shut, and shuts it when it is open, at the
    /// width the strip is now.
    pub(crate) fn fold_split(&mut self, g: &Geometry, t: TrackId, id: NodeId) {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return;
        };
        let nested = flow::nested(track, id);
        let wide = self.width_of(g, t) >= WIDE_W;
        let open = self.mixer.folds.open(id, nested, wide);
        self.mixer.folds.set(id, nested, wide, !open);
    }

    /// Opens a plugin's own window, or shuts it when it is open (UI-025 is
    /// the window's keys; the window itself is the plugin's). A window the
    /// person closed opens again.
    pub fn open_plugin(&mut self, t: TrackId, id: PluginId) -> bool {
        // Asked for again while it loads: it opens once loaded, as it will (PLG-050). Another
        // window asked for drops the wait.
        if let Some(w) = self.mixer.open_later.take()
            && w.id == id
            && !self.has_loaded(id)
        {
            self.mixer.open_later = Some(w);
            return true;
        }
        // The face in the drawer comes out as a window (MIX-046).
        if let Some(i) = self.instruments.dock_slot()
            && self.instruments.open[i].plugin == id
        {
            self.show_instrument(t, id);
            return true;
        }
        if self.mixer.is_open(id) {
            let shown = self.editor_shown(id);
            self.close_editor(id);
            if shown {
                return true;
            }
        }
        if self.instruments.is_open(id) {
            if self.instruments.note.is_some_and(|n| n.plugin == id) {
                self.window_note_off();
            }
            // Its pad's knob still turning goes in while the window still says which pad
            // (L2-ui-1).
            self.device_sweep_end();
            self.instruments.open.retain(|w| w.plugin != id);
            crate::instrument::window::forget_turns(self);
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
                let travel = match h {
                    MixerHit::Fader(_) => self
                        .mixer_rect(g, h)
                        .map_or(100.0, |r| (r.h - CAP_H).max(1.0)),
                    _ => KNOB_TRAVEL,
                };
                self.drag_open(h, y, travel)
            }
            // A track head's fader (UI-324) is the same fader, dragged sideways.
            Hit::HeadFader(t) => {
                let travel = self
                    .head_fader_rect(g, t)
                    .map_or(100.0, |r| (r.w - crate::head_fader::CAP_W).max(1.0));
                let began = self.drag_open(MixerHit::Fader(t), y, travel);
                if self.mixer.drag.is_some() {
                    self.head_fader.dragging = Some(t);
                }
                began
            }
            _ => false,
        }
    }

    /// Takes hold of `h` with the pointer at `y0` (the x of a head's fader), `travel` px from
    /// one end of its range to the other. True when it took the press.
    pub(crate) fn drag_open(&mut self, h: MixerHit, y0: f32, travel: f32) -> bool {
        // Its lane plays it (UI-063): taken, so nothing under it gets the press.
        if self.mixer_locked(h) {
            return true;
        }
        let Some(start) = self.mixer_value(h) else {
            return false;
        };
        self.mixer.drag = Some(Drag {
            hit: h,
            y0,
            travel,
            start,
            value: start,
            alone: self.arrange.mods.alt,
        });
        self.auto_touch(h, Phase::Begin, Some(start));
        true
    }

    /// Starts dragging a macro knob of a window (MIX-041) as a mixer knob: it is one step when
    /// let go. False when it does not move.
    pub(crate) fn control_drag(&mut self, t: TrackId, c: ControlId, y: f32) -> bool {
        let hit = MixerHit::Control(t, c);
        if self.mixer_locked(hit) {
            return false;
        }
        let Some(start) = self.mixer_value(hit) else {
            return false;
        };
        self.mixer.drag = Some(Drag {
            hit,
            y0: y,
            travel: KNOB_TRAVEL,
            start,
            value: start,
            alone: false,
        });
        self.auto_touch(hit, Phase::Begin, Some(start));
        true
    }

    /// The pointer moved during a drag; `fine` (Shift) moves a quarter as far.
    pub fn drag_move(&mut self, y: f32, fine: bool) -> bool {
        self.drag_move_at(0.0, y, fine)
    }

    /// The pointer moved to (`x`, `y`) during a drag: a track head's fader (UI-324) is dragged
    /// sideways, every other control up and down.
    pub fn drag_move_at(&mut self, x: f32, y: f32, fine: bool) -> bool {
        let Some(d) = self.mixer.drag else {
            return false;
        };
        let moved = if self.head_fader.dragging.is_some() {
            x - d.y0
        } else {
            d.y0 - y
        };
        let dy = moved * if fine { 0.25 } else { 1.0 };
        let value = match d.hit {
            MixerHit::Pan(_) => {
                let v = (d.start as f32 + dy * 2.0 / d.travel).clamp(-1.0, 1.0);
                let v = if v.abs() < 0.03 { 0.0 } else { v };
                f64::from((v * 100.0).round() / 100.0)
            }
            MixerHit::Control(..) => {
                let v = (d.start as f32 + dy / d.travel).clamp(0.0, 1.0);
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
                let hit = group::same_on(d.hit, t);
                let to = self.dragged_to(hit, group::level_of(&p, hit), value);
                apply(&mut p, hit, to);
            }
            let heard = self.auto_heard(&p);
            self.transport.sync(&heard, self.song().dir());
        }
        true
    }

    /// Lets go of a drag without a step: the song it began from has been taken back or replaced,
    /// so its values are not the song's any more.
    pub(crate) fn drag_drop(&mut self) {
        let Some(d) = self.mixer.drag else {
            return;
        };
        self.auto_touch(d.hit, Phase::End, None);
        self.mixer.drag = None;
        self.mixer.knob_at = None;
        self.head_fader.dragging = None;
    }

    /// Ends a drag: one command when the value changed.
    pub fn drag_end(&mut self) -> bool {
        let Some(d) = self.mixer.drag else {
            return false;
        };
        // The tracks are the drag's (Alt, the selection), so it goes once they are known.
        let written = self.auto_touch(d.hit, Phase::End, Some(d.value));
        let relative = self.drag_relative();
        let ids: Vec<_> = self
            .auto_direct_targets(d.hit)
            .iter()
            .map(ToString::to_string)
            .collect();
        self.mixer.drag = None;
        self.head_fader.dragging = None;
        if written {
            return true;
        }
        if d.value == d.start || (relative && d.by() == 0.0) {
            // The player may have heard other values on the way.
            let p = self.project();
            self.transport.sync(&p, self.song().dir());
            return true;
        }
        // Several tracks move by as much as the dragged one, each from its own value (MIX-050).
        match d.hit {
            MixerHit::Fader(_) if relative => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command("track.volume.set", json!({ "ids": ids, "by": d.by() }))
            }
            MixerHit::Fader(_) => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command("track.volume.set", json!({ "ids": ids, "db": d.value }))
            }
            MixerHit::Pan(_) if relative => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command("track.pan.set", json!({ "ids": ids, "by": d.by() }))
            }
            MixerHit::Pan(_) => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command("track.pan.set", json!({ "ids": ids, "pan": d.value }))
            }
            MixerHit::SendLevel(_, s) => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command("send.set", json!({ "id": s.to_string(), "level": d.value }))
            }
            MixerHit::Control(t, c) => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "control.set",
                    json!({ "track": t.to_string(), "control": c.to_string(), "value": d.value }),
                )
            }
            _ => false,
        };
        true
    }

    /// A wheel turn over the mixer: an insert or send list moves by a row,
    /// anywhere else the strips move sideways.
    pub fn mixer_wheel(&mut self, g: &Geometry, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        if !self.mixer_shown() || !self.mixer_body(g).contains(x, y) {
            return false;
        }
        let p = self.project();
        for s in self.mixer_strips(g) {
            if !self.strip_holds(g, &s, x, y) {
                continue;
            }
            let list = if s.inserts.contains(x, y) {
                List::Flow
            } else if s.sends.is_some_and(|r| r.contains(x, y)) {
                List::Sends
            } else {
                continue;
            };
            let Some(track) = p.tracks.get(&s.track) else {
                return true;
            };
            if dy != 0.0 {
                let max = if list == List::Flow {
                    self.flow_max(&s, track)
                } else {
                    list_max(&p, track, list, s.rows)
                };
                let off = self.mixer.offset(s.track, list).min(max);
                let next = if dy > 0.0 {
                    off.saturating_sub(1)
                } else {
                    (off + 1).min(max)
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
