//! The shared title bar of a floating window (UI-322): the power, the kind
//! icon and the «track › device» name, the preset tag «‹ ▾ name ›», a
//! draggable empty stretch, the side-chain seat (UI-336), the channel editor's
//! button, the pin and the close, in this order on one surface. The built-in
//! devices' windows carry it; the channel editor's and the external
//! plug-ins' share it when they come (MIX-042, UI-337).

use gpui::Hsla;
use xirius_design::{FontSize, Layout, Radius, Space, Weight};
use xirius_params::BuiltinDef;

use crate::draw::{Align, Pen, Round, estimated_text_width};
use crate::paint::Colors;
use crate::studio::Rect;

/// The bar's height and the side of a square icon button on it.
pub const BAR_H: f32 = 36.0;
pub const BTN: f32 = 28.0;

const PAD: f32 = 4.0;
const GAP: f32 = 2.0;
const ICON: f32 = Layout::ICON_SIZE;

/// Under this bar width the ‹ › of the preset group hide and the name keeps
/// less room.
pub const NARROW: f32 = 380.0;

/// The name group's inner padding (left of the icon, right of the text), the
/// gap between the icon and the text and the ›'s room on each side.
const NAME_L: f32 = 6.0;
const NAME_R: f32 = 4.0;
const NAME_GAP: f32 = 6.0;
const SEP_PAD: f32 = 4.0;

/// The room the rest of the bar always keeps; the name caps at the bar less
/// it (less still under [`NARROW`]), but never below the device's own name
/// while the tag keeps its smallest width.
const NAME_ROOM: f32 = 268.0;
const NAME_ROOM_NARROW: f32 = 220.0;

/// How far Inter's Semibold runs past [`estimated_text_width`] at most: the
/// device's name is held at its real width («Compressor» is 83.5 at 14 for
/// an estimate of 75.3).
const SEMIBOLD_RUN: f32 = 1.12;

/// The preset group «‹ tag ›»: its left margin, its width it shrinks from
/// and to, and a ‹ › button's width. Narrow, the tag gives up to two wide
/// characters so the device keeps its name.
const GROUP_ML: f32 = 6.0;
const GROUP_BASIS: f32 = 220.0;
const GROUP_MIN: f32 = 120.0;
const GROUP_MIN_NARROW: f32 = 60.0;
const STEP_W: f32 = 24.0;

/// Inside the tag: the left padding, the ▾, the name and the dot that says
/// the values moved off it, the right padding. The dot stands a space step
/// off the name.
const TAG_L: f32 = 6.0;
const TAG_R: f32 = 8.0;
const TAG_GAP: f32 = 4.0;
const DOT_GAP: f32 = Space::X2.px();
const CHEV: f32 = 12.0;
const DOT: f32 = 6.0;

/// The draggable empty stretch's smallest width. Narrow it gives all of it
/// to the name, which drags the window too.
const SPACE_MIN: f32 = 8.0;
const SPACE_MIN_NARROW: f32 = 0.0;

/// The side-chain seat (UI-336): the on button around its 16 icon, then the
/// source button «name ▾» with its padding, the gap before the ▾ and the ▾,
/// the source button at most [`SC_SRC_MAX`] wide.
const SC_ON_W: f32 = 6.0 + ICON + 6.0;
const SC_SRC_L: f32 = 4.0;
const SC_SRC_R: f32 = 5.0;
const SC_SRC_GAP: f32 = 4.0;
const SC_SRC_MAX: f32 = 110.0;
/// What the estimate of the name's width can fall short of the drawn text.
const SC_SRC_SLACK: f32 = 8.0;

/// Which of the bar's seats a window has. The channel editor's own window
/// has no channel button; a device that takes no side-chain key has no
/// side-chain seat.
#[derive(Clone, Copy, Debug, Default)]
pub struct Seats<'a> {
    pub power: bool,
    pub sidechain: bool,
    pub channel: bool,
    /// The side-chain seat's source name ([`source_name`]): empty while
    /// nothing keys the device.
    pub source: &'a str,
}

/// The seat's name for a device's sources: the first one's, «+n» for the
/// rest ("Kick +1"); empty with none.
pub fn source_name(first: Option<&str>, count: usize) -> String {
    match (first, count) {
        (None, _) | (_, 0) => String::new(),
        (Some(name), 1) => name.to_string(),
        (Some(name), n) => format!("{name} +{}", n - 1),
    }
}

/// Where the parts of a bar over `r` lie. The name's band and the space are
/// the window's drag handle; the seats of parts a window does not have are
/// `None`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bar {
    pub power: Option<Rect>,
    /// The name group: the kind icon and «track › device».
    pub name: Rect,
    /// The text's zone inside [`Bar::name`], right of the icon.
    pub name_text: Rect,
    /// The ‹ › stepping the presets: `None` while the bar is [`NARROW`].
    pub prev: Option<Rect>,
    pub next: Option<Rect>,
    /// The tag «▾ name ●» that opens the preset list.
    pub tag: Rect,
    /// The tag's ▾: `None` while the bar is [`NARROW`].
    pub chevron: Option<Rect>,
    /// The name's zone inside the tag.
    pub tag_text: Rect,
    /// The side-chain seat, whole: its face while something keys the device.
    pub sidechain: Option<Rect>,
    /// The seat's on button (the device's sidechain on or off) and its
    /// source button «name ▾» that opens the source list.
    pub sidechain_on: Option<Rect>,
    pub sidechain_list: Option<Rect>,
    /// The source name's zone in the source button: `None` with no source or
    /// while the bar is [`NARROW`].
    pub sidechain_name: Option<Rect>,
    /// The channel editor's button (MIX-042 opens its window).
    pub channel: Option<Rect>,
    /// The always-empty draggable stretch.
    pub space: Rect,
    pub pin: Rect,
    pub close: Rect,
}

/// The built-ins whose bar keeps a side-chain seat (the report's «受ける
/// 機器»: the compressor, the limiter, the gate when it comes).
pub fn takes_sidechain(def: &BuiltinDef) -> bool {
    matches!(def.uid, "compressor" | "limiter" | "gate")
}

/// Lays the parts of a bar over `r` out, `track` and `device` feeding the
/// name group's width. The buttons never give; the group does before the
/// name, the empty stretch last but never below its smallest width. The
/// name keeps room for the device alone while the group keeps its smallest.
pub fn layout(r: Rect, track: &str, device: &str, seats: Seats<'_>) -> Bar {
    let narrow = r.w < NARROW;
    let cy = r.y + (r.h - BTN) / 2.0;
    let btn = |x: f32| Rect::new(x, cy, BTN, BTN);
    let mut x = r.x + PAD;

    let power = seats.power.then(|| {
        let b = btn(x);
        x += BTN + GAP;
        b
    });

    // The right end, fixed: close, pin, the channel editor's, the seat.
    let mut rx = r.right() - PAD;
    let close = Rect::new(rx - BTN, cy, BTN, BTN);
    rx -= BTN + GAP;
    let pin = Rect::new(rx - BTN, cy, BTN, BTN);
    rx -= BTN + GAP;
    let channel = seats.channel.then(|| Rect::new(rx - BTN, cy, BTN, BTN));
    if seats.channel {
        rx -= BTN + GAP;
    }
    // The seat: the on button, then «name ▾» (the name gone while narrow).
    let named = !narrow && !seats.source.is_empty();
    let src_w = if named {
        (SC_SRC_L
            + estimated_text_width(seats.source, FontSize::Small)
            + SC_SRC_SLACK
            + SC_SRC_GAP
            + CHEV
            + SC_SRC_R)
            .min(SC_SRC_MAX)
    } else {
        SC_SRC_L + CHEV + SC_SRC_R
    };
    let seat_w = SC_ON_W + src_w;
    let sidechain = seats
        .sidechain
        .then(|| Rect::new(rx - seat_w, cy, seat_w, BTN));
    let sidechain_on = sidechain.map(|s| Rect::new(s.x, cy, SC_ON_W, BTN));
    let sidechain_list = sidechain.map(|s| Rect::new(s.x + SC_ON_W, cy, src_w, BTN));
    let sidechain_name = sidechain_list.filter(|_| named).map(|l| {
        Rect::new(
            l.x + SC_SRC_L,
            cy,
            (l.w - SC_SRC_L - SC_SRC_GAP - CHEV - SC_SRC_R).max(0.0),
            BTN,
        )
    });
    let seat_room = if seats.sidechain { seat_w + GAP } else { 0.0 };
    rx -= seat_room;
    let space_min = if narrow { SPACE_MIN_NARROW } else { SPACE_MIN };
    let group_min = if narrow { GROUP_MIN_NARROW } else { GROUP_MIN };

    // The name group's own width, capped so the rest keeps its room; the
    // cap stays at the device's own name while the group keeps its least.
    let room = if narrow { NAME_ROOM_NARROW } else { NAME_ROOM } + seat_room;
    let lead = NAME_L + ICON + NAME_GAP;
    let budget = (rx - x - 2.0 * GAP - GROUP_ML - space_min - group_min).max(0.0);
    let device_w = estimated_text_width(device, FontSize::Body) * SEMIBOLD_RUN;
    let cap = (r.w - room)
        .max((lead + device_w + NAME_R).min(budget))
        .max(0.0);
    let content = lead
        + estimated_text_width(track, FontSize::Body)
        + 2.0 * SEP_PAD
        + estimated_text_width("›", FontSize::Body)
        + device_w
        + NAME_R;
    let name = Rect::new(x, cy, content.min(cap), BTN);
    let name_text = Rect::new(name.x + lead, cy, (name.w - lead - NAME_R).max(0.0), BTN);
    x = name.right() + GAP + GROUP_ML;

    // The group takes what is left, its width between its basis and its
    // minimum; under it the empty stretch starts.
    let group_room = (rx - x - GAP - space_min).max(0.0);
    let group = Rect::new(
        x,
        cy,
        group_room.min(GROUP_BASIS).max(group_min.min(group_room)),
        BTN,
    );
    let step = if narrow { 0.0 } else { STEP_W };
    let prev = (!narrow).then(|| Rect::new(group.x, cy, step, BTN));
    let next = (!narrow).then(|| Rect::new(group.right() - step, cy, step, BTN));
    let tag = Rect::new(group.x + step, cy, group.w - 2.0 * step, BTN);
    // Narrow, the ▾ folds with the ‹ › and the tag's face alone says it
    // opens the list: its room goes to the name and the changed mark.
    let chevron =
        (!narrow).then(|| Rect::new(tag.x + TAG_L, tag.y + (tag.h - CHEV) / 2.0, CHEV, CHEV));
    let lead = TAG_L + if narrow { 0.0 } else { CHEV + TAG_GAP };
    let tag_text = Rect::new(tag.x + lead, cy, (tag.w - lead - TAG_R).max(0.0), BTN);
    let space = Rect::new(
        group.right() + GAP,
        cy,
        (rx - group.right() - GAP).max(0.0),
        BTN,
    );

    Bar {
        power,
        name,
        name_text,
        prev,
        next,
        tag,
        chevron,
        tag_text,
        sidechain,
        sidechain_on,
        sidechain_list,
        sidechain_name,
        channel,
        space,
        pin,
        close,
    }
}

/// What a bar paints with, behind the same [`layout`]'s seats.
pub struct View<'a> {
    /// The «track › device» of the name group.
    pub track: &'a str,
    pub device: &'a str,
    /// The kind icon before it ("piano" an instrument, "ins" an effect,
    /// "route" the channel editor).
    pub icon: &'static str,
    /// Powered on: the power's icon is `ok`, off it is the line's colour.
    pub powered: bool,
    /// The tag's preset name and the changed mark after it.
    pub preset: &'a str,
    pub modified: bool,
    /// The pin is down: the `primary` pair paints it.
    pub pinned: bool,
    /// The device's sidechain is on: the seat's icon takes the mark's ink.
    pub keyed: bool,
    pub seats: Seats<'a>,
}

/// The bar over `r` (the window's head, [`BAR_H`] tall).
pub fn paint(pen: &mut Pen, r: Rect, v: &View<'_>) {
    let c = pen.c;
    let bar = layout(r, v.track, v.device, v.seats);
    // The bar follows the window's top corners.
    pen.shape(r, c.surface_1, Radius::M, Round::rows(true, false));

    if let Some(b) = bar.power {
        pen.icon(
            "power",
            icon_box(b),
            if v.powered { c.ok } else { c.line[0] },
        );
    }

    pen.mask(bar.name, |pen| {
        let icon = Rect::new(bar.name.x + NAME_L, cy_of(bar.name), ICON, ICON);
        let ink = name_inks(c, v.powered);
        pen.icon(v.icon, icon, ink.icon);
        // «track › device»: the track rests, the › sinks, the device leads.
        // The device's name holds out longest: the track cuts first, then
        // goes with the › and the device alone cuts last.
        let t = bar.name_text;
        let sep_w = pen.measure("›", FontSize::Body, Weight::Regular);
        let track_w = pen.measure(v.track, FontSize::Body, Weight::Regular);
        let track_min = v.track.chars().next().map_or(0.0, |ch| {
            pen.measure(&format!("{ch}…"), FontSize::Body, Weight::Regular)
        });
        let dev_w = pen.measure(v.device, FontSize::Body, Weight::Semibold);
        let sep_room = 2.0 * SEP_PAD + sep_w;
        let track = match name_fit(track_w, track_min, sep_room, dev_w, t.w) {
            NameFit::Both => v.track.to_string(),
            NameFit::TrackCut(room) => pen.fit(v.track, FontSize::Body, Weight::Regular, room),
            NameFit::DeviceOnly => {
                let device = pen.fit(v.device, FontSize::Body, Weight::Semibold, t.w);
                pen.text(
                    &device,
                    FontSize::Body,
                    Weight::Semibold,
                    ink.device,
                    t,
                    Align::Left,
                );
                return;
            }
        };
        let mut x = t.x
            + pen.text(
                &track,
                FontSize::Body,
                Weight::Regular,
                ink.track,
                t,
                Align::Left,
            );
        x += SEP_PAD;
        let sep = Rect::new(x, t.y, sep_w, t.h);
        pen.text(
            "›",
            FontSize::Body,
            Weight::Regular,
            ink.track,
            sep,
            Align::Left,
        );
        let dev = Rect::new(
            sep.right() + SEP_PAD,
            t.y,
            (t.right() - sep.right() - SEP_PAD).max(0.0),
            t.h,
        );
        pen.text(
            v.device,
            FontSize::Body,
            Weight::Semibold,
            ink.device,
            dev,
            Align::Left,
        );
    });

    // ‹ › and the tag: a filled face of s2, the preset's name and the mark
    // when the values moved off it.
    if let Some(b) = bar.prev {
        pen.icon("chevron-left", icon_box(b), c.ink);
    }
    if let Some(b) = bar.next {
        pen.icon("chevron-right", icon_box(b), c.ink);
    }
    pen.round(bar.tag, c.surface_2, Radius::S);
    if let Some(chev) = bar.chevron {
        pen.icon("chevron-down", chev, c.ink);
    }
    pen.mask(bar.tag, |pen| {
        let room = preset_room(bar.tag_text, v.modified);
        let shown = pen.fit(v.preset, FontSize::Body, Weight::Regular, room);
        let shown_w = pen.text(
            &shown,
            FontSize::Body,
            Weight::Regular,
            c.ink_strong,
            bar.tag_text,
            Align::Left,
        );
        if v.modified {
            pen.round(
                Rect::new(
                    dot_x(bar.tag_text, shown_w),
                    bar.tag.y + (bar.tag.h - DOT) / 2.0,
                    DOT,
                    DOT,
                ),
                c.ink_mark,
                Radius::Pill,
            );
        }
    });

    // The side-chain seat (UI-336); the empty stretch paints nothing.
    if let (Some(seat), Some(on), Some(list)) =
        (bar.sidechain, bar.sidechain_on, bar.sidechain_list)
    {
        let has = !v.seats.source.is_empty();
        let ink = seat_inks(c, has, v.keyed);
        if has {
            pen.round(seat, c.surface_3, Radius::S);
        }
        pen.icon("sc", icon_box(on), ink.icon);
        if let Some(t) = bar.sidechain_name {
            let shown = pen.fit(v.seats.source, FontSize::Small, Weight::Regular, t.w);
            pen.text(
                &shown,
                FontSize::Small,
                Weight::Regular,
                ink.name,
                t,
                Align::Left,
            );
        }
        let chev = Rect::new(
            list.right() - SC_SRC_R - CHEV,
            list.y + (list.h - CHEV) / 2.0,
            CHEV,
            CHEV,
        );
        pen.icon("chevron-down", chev, ink.chevron);
    }
    if let Some(b) = bar.channel {
        pen.icon("route", icon_box(b), c.ink);
    }
    if v.pinned {
        let (face, ink) = c.primary;
        pen.round(bar.pin, face, Radius::S);
        pen.icon("pin", icon_box(bar.pin), ink);
    } else {
        pen.icon("pin", icon_box(bar.pin), c.ink);
    }
    pen.icon("close", icon_box(bar.close), c.ink);
}

/// The channel editor's bar (MIX-042): the route icon and the track's name, the empty
/// stretch, the pin and the close. No power, no preset (PLG-065 brings its tag). The macro
/// window's (MIX-041) is the same with «track › Macro Controls» and the channel editor's
/// button before the pin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlainBar {
    pub name: Rect,
    pub name_text: Rect,
    pub space: Rect,
    pub channel: Option<Rect>,
    pub pin: Rect,
    pub close: Rect,
}

pub fn plain_layout(r: Rect, track: &str) -> PlainBar {
    plain_layout_of(r, track, None)
}

/// The plain bar with `device` after the track's name, and then the channel editor's button.
pub fn plain_layout_of(r: Rect, track: &str, device: Option<&str>) -> PlainBar {
    let cy = r.y + (r.h - BTN) / 2.0;
    let close = Rect::new(r.right() - PAD - BTN, cy, BTN, BTN);
    let pin = Rect::new(close.x - GAP - BTN, cy, BTN, BTN);
    let channel = device.map(|_| Rect::new(pin.x - GAP - BTN, cy, BTN, BTN));
    let right = channel.unwrap_or(pin).x;
    let x = r.x + PAD;
    let lead = NAME_L + ICON + NAME_GAP;
    let room = (right - GAP - x).max(0.0);
    let text = match device {
        Some(d) => {
            estimated_text_width(track, FontSize::Body)
                + 2.0 * SEP_PAD
                + estimated_text_width("›", FontSize::Body)
                + estimated_text_width(d, FontSize::Body) * SEMIBOLD_RUN
        }
        None => estimated_text_width(track, FontSize::Body) * SEMIBOLD_RUN,
    };
    let name = Rect::new(x, cy, (lead + text + NAME_R).min(room), BTN);
    let name_text = Rect::new(name.x + lead, cy, (name.w - lead - NAME_R).max(0.0), BTN);
    let space = Rect::new(
        name.right() + GAP,
        cy,
        (right - GAP - name.right() - GAP).max(0.0),
        BTN,
    );
    PlainBar {
        name,
        name_text,
        space,
        channel,
        pin,
        close,
    }
}

pub fn paint_plain(pen: &mut Pen, r: Rect, track: &str, pinned: bool) {
    paint_plain_of(pen, r, "route", track, None, pinned);
}

/// The plain bar of [`plain_layout_of`]: «track › device» as a device's bar says it.
pub fn paint_plain_of(
    pen: &mut Pen,
    r: Rect,
    icon: &str,
    track: &str,
    device: Option<&str>,
    pinned: bool,
) {
    let c = pen.c;
    let bar = plain_layout_of(r, track, device);
    pen.shape(r, c.surface_1, Radius::M, Round::rows(true, false));
    pen.mask(bar.name, |pen| {
        let at = Rect::new(bar.name.x + NAME_L, cy_of(bar.name), ICON, ICON);
        pen.icon(icon, at, c.ink_strong);
        let t = bar.name_text;
        let Some(device) = device else {
            let shown = pen.fit(track, FontSize::Body, Weight::Semibold, t.w);
            pen.text(
                &shown,
                FontSize::Body,
                Weight::Semibold,
                c.ink_strong,
                t,
                Align::Left,
            );
            return;
        };
        let ink = name_inks(c, true);
        let sep_w = pen.measure("›", FontSize::Body, Weight::Regular);
        let dev_w = pen.measure(device, FontSize::Body, Weight::Semibold);
        let track_room = (t.w - dev_w - 2.0 * SEP_PAD - sep_w).max(0.0);
        let track = pen.fit(track, FontSize::Body, Weight::Regular, track_room);
        let track_w = pen.measure(&track, FontSize::Body, Weight::Regular);
        pen.text(
            &track,
            FontSize::Body,
            Weight::Regular,
            ink.track,
            Rect::new(t.x, t.y, track_w, t.h),
            Align::Left,
        );
        let sx = t.x + track_w + SEP_PAD;
        pen.text(
            "›",
            FontSize::Body,
            Weight::Regular,
            ink.track,
            Rect::new(sx, t.y, sep_w, t.h),
            Align::Left,
        );
        let dx = sx + sep_w + SEP_PAD;
        pen.text(
            device,
            FontSize::Body,
            Weight::Semibold,
            ink.device,
            Rect::new(dx, t.y, (t.right() - dx).max(0.0), t.h),
            Align::Left,
        );
    });
    if let Some(b) = bar.channel {
        pen.icon("route", icon_box(b), c.ink);
    }
    if pinned {
        let (face, ink) = c.primary;
        pen.round(bar.pin, face, Radius::S);
        pen.icon("pin", icon_box(bar.pin), ink);
    } else {
        pen.icon("pin", icon_box(bar.pin), c.ink);
    }
    pen.icon("close", icon_box(bar.close), c.ink);
}

/// How «track › device» goes into the name's zone.
#[derive(Clone, Copy, Debug, PartialEq)]
enum NameFit {
    /// Both names whole.
    Both,
    /// The track cut into the room it has; the › and the device whole.
    TrackCut(f32),
    /// The device alone, cut to an ellipsis if even it does not fit.
    DeviceOnly,
}

/// Fits the measured «track › device» into `room`: the track gives first,
/// down to its first character and «…» (`track_min`), then goes with the ›.
fn name_fit(track_w: f32, track_min: f32, sep_room: f32, dev_w: f32, room: f32) -> NameFit {
    if track_w + sep_room + dev_w <= room {
        NameFit::Both
    } else if track_min + sep_room + dev_w <= room {
        NameFit::TrackCut(room - sep_room - dev_w)
    } else {
        NameFit::DeviceOnly
    }
}

/// The name group's inks.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NameInks {
    icon: Hsla,
    track: Hsla,
    device: Hsla,
}

/// On, the kind icon and the device lead in the strong ink and the track
/// rests in the body's. Off, the icon takes the power's off colour and the
/// text the blue-grey ink: still readable on s1 (7:1 and over) but plainly
/// apart from the strong (the body's is only 1.2:1 from it).
fn name_inks(c: &Colors, powered: bool) -> NameInks {
    if powered {
        NameInks {
            icon: c.ink_strong,
            track: c.ink,
            device: c.ink_strong,
        }
    } else {
        NameInks {
            icon: c.line[0],
            track: c.ink_blue,
            device: c.ink_blue,
        }
    }
}

/// The side-chain seat's inks.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SeatInks {
    icon: Hsla,
    name: Hsla,
    chevron: Hsla,
}

/// Keyed and on, the icon takes the mark's ink and the name the strong; keyed
/// but off, the icon the power's off colour and the name the blue-grey ink,
/// as a device's name off; with no source, the body's ink.
fn seat_inks(c: &Colors, has: bool, on: bool) -> SeatInks {
    match (has, on) {
        (true, true) => SeatInks {
            icon: c.ink_mark,
            name: c.ink_strong,
            chevron: c.ink_strong,
        },
        (true, false) => SeatInks {
            icon: c.line[0],
            name: c.ink_blue,
            chevron: c.ink_strong,
        },
        (false, _) => SeatInks {
            icon: c.ink,
            name: c.ink,
            chevron: c.ink,
        },
    }
}

/// The preset name's room inside the tag: with the changed mark, the dot and
/// its space step after the name come off it so the dot never covers it.
fn preset_room(tag_text: Rect, modified: bool) -> f32 {
    if modified {
        (tag_text.w - DOT_GAP - DOT).max(0.0)
    } else {
        tag_text.w
    }
}

/// The changed mark's left edge after a name drawn `shown_w` wide.
fn dot_x(tag_text: Rect, shown_w: f32) -> f32 {
    tag_text.x + shown_w + DOT_GAP
}

/// The 16 icon's box inside a part.
fn icon_box(b: Rect) -> Rect {
    Rect::new(
        b.x + (b.w - ICON) / 2.0,
        b.y + (b.h - ICON) / 2.0,
        ICON,
        ICON,
    )
}

/// The part's vertical middle for an `ICON`-tall box.
fn cy_of(b: Rect) -> f32 {
    b.y + (b.h - ICON) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEATS: Seats<'static> = Seats {
        power: true,
        sidechain: true,
        channel: true,
        source: "Kick +1",
    };

    /// Every part is inside the bar and none of them overlap.
    fn inside(b: &Bar, r: Rect) {
        for q in [
            b.power,
            Some(b.name),
            b.prev,
            b.next,
            Some(b.tag),
            b.sidechain,
            b.channel,
            Some(b.pin),
            Some(b.close),
        ]
        .into_iter()
        .flatten()
        {
            assert!(
                q.x >= r.x - 0.01 && q.right() <= r.right() + 0.01,
                "{q:?} in {r:?}"
            );
            assert!(
                q.y >= r.y - 0.01 && q.bottom() <= r.bottom() + 0.01,
                "{q:?} in {r:?}"
            );
        }
    }

    #[test]
    fn the_parts_are_in_their_order_and_fit_the_bar() {
        for w in [1280.0, 416.0, 360.0, 316.0] {
            let r = Rect::new(0.0, 0.0, w, BAR_H);
            let b = layout(r, "Vocal", "Compressor", SEATS);
            inside(&b, r);
            // The order the bar reads: power, name, ‹ › tag, space,
            // side-chain seat, channel editor, pin, close.
            assert_eq!(b.power.unwrap().x, r.x + PAD);
            assert!(b.power.unwrap().right() <= b.name.x + GAP + 0.01);
            assert!(b.name.right() < b.tag.x + 0.01, "{w}");
            assert!(b.tag.right() <= b.space.x + GAP + 0.01);
            let sidechain = b.sidechain.unwrap();
            assert!(b.space.right() <= sidechain.x + 0.01);
            assert!(sidechain.right() + GAP <= b.channel.unwrap().x + 0.01);
            assert!(b.channel.unwrap().right() <= b.pin.x + GAP + 0.01);
            assert!(b.pin.right() <= b.close.x + GAP + 0.01);
            assert_eq!(b.close.right(), r.right() - PAD);
            // The buttons are 28s and keep their width at any size.
            for q in [b.power.unwrap(), b.tag, b.pin, b.close] {
                assert_eq!(q.h, BTN);
            }
            assert_eq!(b.pin.w, BTN);
            assert_eq!(b.close.w, BTN);
            assert_eq!(b.power.unwrap().w, BTN);
        }
    }

    #[test]
    fn the_steps_hide_and_the_tag_shrinks_under_the_narrow_width() {
        let wide = layout(
            Rect::new(0.0, 0.0, NARROW, BAR_H),
            "Vocal",
            "Compressor",
            SEATS,
        );
        assert!(wide.prev.is_some() && wide.next.is_some());
        let just = layout(
            Rect::new(0.0, 0.0, NARROW - 1.0, BAR_H),
            "Vocal",
            "Compressor",
            SEATS,
        );
        assert!(just.prev.is_none() && just.next.is_none());
        assert!(just.tag.w >= GROUP_MIN_NARROW);
        assert!(wide.chevron.is_some());
        assert!(just.chevron.is_none(), "the ▾ folds with the ‹ ›");
    }

    #[test]
    fn the_name_gives_its_room_and_the_space_stays() {
        let r = Rect::new(0.0, 0.0, 316.0, BAR_H);
        let b = layout(
            r,
            "A rather long track name for a test",
            "A long device name too",
            SEATS,
        );
        inside(&b, r);
        // The name stops where the tag keeps its least; nothing overlaps.
        assert!(b.tag.w >= GROUP_MIN_NARROW - 0.01, "{:?}", b.tag);
        assert!(b.name.right() + GAP + GROUP_ML <= b.tag.x + 0.01);
        assert!(b.space.w >= SPACE_MIN_NARROW, "{:?}", b.space);
        assert!(b.tag.right() <= b.space.x + 0.01);
        // Wide, the stretch keeps its least.
        let r = Rect::new(0.0, 0.0, 1280.0, BAR_H);
        let b = layout(
            r,
            "A rather long track name for a test",
            "Compressor",
            SEATS,
        );
        assert!(b.space.w >= SPACE_MIN - 0.01, "{:?}", b.space);
    }

    /// Inter's Semibold at 14 measured off the bundled font: the narrow
    /// built-ins' names and «Compressor», the widest of them.
    const SEMIBOLD_14: [(&str, f32); 6] = [
        ("Compressor", 83.5),
        ("Limiter", 46.6),
        ("Delay", 38.3),
        ("Reverb", 48.2),
        ("Sampler", 56.0),
        ("Drum Kit", 58.8),
    ];

    #[test]
    fn the_narrow_windows_keep_the_device_name_whole() {
        // The 3-column windows are 316 wide and take no key: no seat. (A
        // seat's window cuts the name at its narrowest, as the mock does.)
        {
            let seats = Seats {
                sidechain: false,
                ..SEATS
            };
            for (device, w) in SEMIBOLD_14 {
                let r = Rect::new(0.0, 0.0, 316.0, BAR_H);
                let b = layout(r, "Vocal", device, seats);
                inside(&b, r);
                assert!(b.name_text.w >= w, "{device} ({w}) in {:?}", b.name_text);
                // The tag keeps two 14 wide characters readable.
                assert!(b.tag_text.w >= 28.0, "{device}: {:?}", b.tag_text);
            }
        }
    }

    #[test]
    fn the_track_gives_before_the_device() {
        // Measured Regular at 14: «Vocal» 37.3, «›» 5.4, «V…» 21.8.
        let sep_room = 2.0 * SEP_PAD + 5.4;
        let (track_w, track_min) = (37.3, 21.8);
        // The 316 wide windows take no key: no seat.
        let seats = Seats {
            sidechain: false,
            ..SEATS
        };
        let name_room = |device: &str| {
            let r = Rect::new(0.0, 0.0, 316.0, BAR_H);
            layout(r, "Vocal", device, seats).name_text.w
        };
        for (device, dev_w) in SEMIBOLD_14 {
            let fit = name_fit(track_w, track_min, sep_room, dev_w, name_room(device));
            match fit {
                NameFit::Both => {}
                NameFit::TrackCut(room) => assert!(room >= track_min && room < track_w),
                // The device alone is whole: it fits the room by the test above.
                NameFit::DeviceOnly => assert!(dev_w <= name_room(device)),
            }
        }
        assert_eq!(
            name_fit(track_w, track_min, sep_room, 83.5, 84.0),
            NameFit::DeviceOnly,
            "«Compressor» at 316 goes alone rather than cut"
        );
        assert_eq!(
            name_fit(track_w, track_min, sep_room, 48.2, 90.0),
            NameFit::TrackCut(90.0 - sep_room - 48.2)
        );
        assert_eq!(
            name_fit(track_w, track_min, sep_room, 48.2, 200.0),
            NameFit::Both
        );
    }

    #[test]
    fn the_name_of_a_device_off_reads_apart_and_still_reads() {
        use xirius_design::{Ink, MIN_TEXT_CONTRAST, Readable, Surface, Theme, contrast_ratio};
        for theme in [Theme::Dark, Theme::Light] {
            let c = Colors::new(theme);
            let on = name_inks(&c, true);
            let off = name_inks(&c, false);
            assert_ne!(on.device, off.device);
            assert_ne!(on.track, off.track);
            assert_eq!(off.icon, c.line[0], "the icon goes as the power's off");
            assert_eq!(off.device, c.ink_blue);
            // The off ink keeps the text's contrast on s1 and stands apart
            // from the strong far more than the body's does.
            let s1 = Surface::S1.color(theme);
            let strong = Surface::S1.strong_text().ink_color(theme);
            let body = Surface::S1.text().ink_color(theme);
            let blue = Readable::new(Surface::S1, Ink::Blue)
                .expect("Blue is readable on S1")
                .ink_color(theme);
            assert!(contrast_ratio(s1, blue) >= MIN_TEXT_CONTRAST, "{theme:?}");
            assert!(
                contrast_ratio(strong, blue) >= 1.5 * contrast_ratio(strong, body),
                "{theme:?}"
            );
        }
    }

    #[test]
    fn the_changed_mark_stands_a_space_off_the_name() {
        let r = Rect::new(0.0, 0.0, 316.0, BAR_H);
        let b = layout(r, "Vocal", "Compressor", SEATS);
        let t = b.tag_text;
        let room = preset_room(t, true);
        assert_eq!(room, t.w - DOT_GAP - DOT);
        assert_eq!(preset_room(t, false), t.w);
        // Two wide characters keep whole with the mark even at 316.
        assert!(room >= 28.0, "{t:?}");
        // Whatever the shown width up to the room, the dot keeps the gap
        // and stays inside the name's zone.
        for shown in [0.0, 12.0, room / 2.0, room] {
            let x = dot_x(t, shown);
            assert_eq!(x - (t.x + shown), Space::X2.px());
            assert!(x + DOT <= t.right() + 0.01, "{shown}");
        }
    }

    #[test]
    fn the_seat_holds_the_on_button_and_the_source_button() {
        let wide = Rect::new(0.0, 0.0, 680.0, BAR_H);
        let b = layout(wide, "Bass", "Compressor", SEATS);
        let (seat, on, list) = (
            b.sidechain.unwrap(),
            b.sidechain_on.unwrap(),
            b.sidechain_list.unwrap(),
        );
        assert_eq!((on.x, on.w, on.h), (seat.x, SC_ON_W, BTN));
        assert_eq!(list.x, on.right());
        assert_eq!(list.right(), seat.right());
        let name = b.sidechain_name.unwrap();
        assert!(name.x >= list.x && name.right() + CHEV <= list.right());
        assert!(name.w >= estimated_text_width("Kick +1", FontSize::Small) - 0.01);
        // A long name stops at the source button's most.
        let long = Seats {
            source: "A very long track name +3",
            ..SEATS
        };
        let b = layout(wide, "Bass", "Compressor", long);
        assert_eq!(b.sidechain_list.unwrap().w, SC_SRC_MAX);
        // No source, or narrow: «▾» alone.
        let none = Seats {
            source: "",
            ..SEATS
        };
        let bare = SC_SRC_L + CHEV + SC_SRC_R;
        let b = layout(wide, "Bass", "Compressor", none);
        assert!(b.sidechain_name.is_none());
        assert_eq!(b.sidechain_list.unwrap().w, bare);
        for w in [360.0, 320.0] {
            let r = Rect::new(0.0, 0.0, w, BAR_H);
            let b = layout(r, "Bass", "Compressor", SEATS);
            inside(&b, r);
            assert!(b.sidechain_name.is_none(), "{w}");
            assert_eq!(b.sidechain_list.unwrap().w, bare);
            // the name and the tag keep clear of it, the tag two characters
            assert!(b.tag.right() <= b.sidechain.unwrap().x + 0.01, "{w}");
            assert!(b.tag_text.w >= 28.0, "{w}: {:?}", b.tag_text);
        }
    }

    #[test]
    fn the_source_name_counts_the_rest() {
        assert_eq!(source_name(None, 0), "");
        assert_eq!(source_name(Some("Kick"), 1), "Kick");
        assert_eq!(source_name(Some("Kick"), 3), "Kick +2");
    }

    #[test]
    fn the_seat_reads_on_off_and_empty_apart() {
        use xirius_design::{Ink, MIN_TEXT_CONTRAST, Readable, Surface, Theme, contrast_ratio};
        for theme in [Theme::Dark, Theme::Light] {
            let c = Colors::new(theme);
            let on = seat_inks(&c, true, true);
            let idle = seat_inks(&c, true, false);
            let empty = seat_inks(&c, false, false);
            assert_eq!(on.icon, c.ink_mark);
            assert_eq!(idle.icon, c.line[0], "off as the power's off");
            assert_ne!(on.name, idle.name);
            assert_eq!(empty.icon, c.ink);
            // the idle name keeps the text's contrast on the seat's s3 face
            let blue = Readable::new(Surface::S3, Ink::Blue)
                .expect("Blue is readable on S3")
                .ink_color(theme);
            assert!(
                contrast_ratio(Surface::S3.color(theme), blue) >= MIN_TEXT_CONTRAST,
                "{theme:?}"
            );
        }
    }

    #[test]
    fn a_window_without_the_seats_lays_the_rest() {
        let seats = Seats {
            power: false,
            sidechain: false,
            channel: false,
            source: "",
        };
        let r = Rect::new(0.0, 0.0, 416.0, BAR_H);
        let b = layout(r, "Vocal", "EQ", seats);
        inside(&b, r);
        assert!(b.power.is_none() && b.sidechain.is_none() && b.channel.is_none());
        assert_eq!(b.name.x, r.x + PAD, "the name leads when there is no power");
    }
}
