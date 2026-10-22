//! Gesture contexts share the hit names with the map and the settings preview.
use crate::tools::Mods;
use xirius_prefs::Mousemap;

impl crate::studio::Studio {
    pub fn mouse_modifiers(
        &self,
        spot: &crate::arrange::Spot,
        tool: crate::tools::Tool,
        raw: Mods,
        during: bool,
    ) -> Mods {
        let Some(base) = gesture_base(spot, tool) else {
            return raw;
        };
        let context = format!("{base}.drag");
        let s = self
            .song
            .settings()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let first = modifiers(&s.mousemap, &context, raw, false);
        if during {
            self::during(&s.mousemap, &context, raw, first)
        } else {
            first
        }
    }

    /// `select.after` on the arrangement (KEY-016): the clip under the pointer, if any, and every
    /// clip on any track that starts at or after the pointer's tick. The roll's version takes the
    /// notes after the pointer the same way.
    pub(crate) fn select_clips_after(
        &mut self,
        tick: xirius_core::time::Tick,
        under: Option<(xirius_core::id::TrackId, xirius_core::id::ClipId)>,
    ) {
        let p = self.project();
        let mut picked: std::collections::BTreeSet<_> = p
            .tracks
            .values()
            .flat_map(|t| t.clips.iter().map(move |c| (t.id, c.id, c.start)))
            .filter(|(_, _, start)| *start >= tick)
            .map(|(t, c, _)| (t, c))
            .collect();
        picked.extend(under);
        self.selected_clip = under.or_else(|| picked.iter().next().copied());
        if let Some((t, _)) = under {
            self.select_row(Some(crate::tree::RowKind::Track(t)));
        }
        self.arrange.clips = picked;
        self.arrange.touched();
    }

    /// What the map gives a press on `spot`: the click's action and the drag's, `Some("")` for
    /// "nothing". None where the spot is not one of the map's hit areas.
    pub fn mouse_actions(
        &self,
        spot: &crate::arrange::Spot,
        tool: crate::tools::Tool,
        raw: Mods,
    ) -> Option<(Option<String>, Option<String>)> {
        let base = gesture_base(spot, tool)?;
        let s = self
            .song
            .settings()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Some((
            action(&s.mousemap, &format!("{base}.click"), raw),
            action(&s.mousemap, &format!("{base}.drag"), raw),
        ))
    }
}

/// `<area>.<tool>.<zone>` of a press in the arrangement: the clips' and the automation lanes'.
pub fn gesture_base(spot: &crate::arrange::Spot, tool: crate::tools::Tool) -> Option<String> {
    if let Some(zone) = arrange_zone(spot) {
        return Some(format!("arrange.{}.{}", tool.name(), zone));
    }
    let zone = automation_zone(spot)?;
    Some(format!("automation.{}.{zone}", automation_tool(tool)))
}

/// An explicit empty binding consumes the gesture. Missing entries keep the area's tool.
pub fn action(map: &Mousemap, context: &str, raw: Mods) -> Option<String> {
    let modifier = xirius_prefs::Chord::mods_text(crate::input_dispatch::bits(raw));
    map.changed
        .get(context)
        .and_then(|rows| rows.get(&modifier))
        .cloned()
        .or_else(|| map.table(context).get(&modifier).cloned())
}

pub fn during(map: &Mousemap, context: &str, raw: Mods, mut mapped: Mods) -> Mods {
    if !map.changed.contains_key(context) {
        return mapped;
    }
    let marks = modifiers(map, context, raw, true);
    mapped.shift = marks.shift;
    let key = format!(
        "during.{}",
        xirius_prefs::Chord::mods_text(crate::input_dispatch::bits(raw))
    );
    if map
        .table(context)
        .get(&key)
        .is_some_and(|a| a.starts_with("note.resize --") || a == "note.strum")
    {
        mapped.ctrl = marks.ctrl;
        mapped.alt = marks.alt;
    }
    // A point moved with its time (or its axis) held is whatever modifier the map gives `--lock`.
    if context.starts_with("lane.") || context.starts_with("automation.") {
        mapped.ctrl = locks(map, context, raw);
    }
    mapped
}

/// Whether a `--lock` row of the drag's table is held (a modifier it names is down).
fn locks(map: &Mousemap, context: &str, raw: Mods) -> bool {
    let held = crate::input_dispatch::bits(raw);
    map.table(context).iter().any(|(m, a)| {
        a.ends_with("--lock")
            && m.strip_prefix("during.")
                .and_then(|m| xirius_prefs::Chord::parse_mods(m, xirius_prefs::Os::Win).ok())
                .is_some_and(|b| b != 0 && held & b == b)
    })
}

/// Translate a configured gesture to the modifiers the drag preview consumes. Tool holds
/// are resolved separately, before the gesture starts. Each release still commits once.
pub fn modifiers(map: &Mousemap, context: &str, raw: Mods, during: bool) -> Mods {
    let rows = map.table(context);
    let prefix = if during { "during." } else { "" };
    let key = format!(
        "{prefix}{}",
        xirius_prefs::Chord::mods_text(crate::input_dispatch::bits(raw))
    );
    let action = rows.get(&key).map(String::as_str);
    // During markers are evaluated individually too, so Shift and Alt can be combined.
    if during {
        let mut out = raw;
        let custom = map.changed.get(context);
        if custom.is_none() {
            return raw;
        }
        out.shift = rows.iter().any(|(m, a)| {
            a == "snap.invert"
                && m.starts_with("during.")
                && xirius_prefs::Chord::parse_mods(&m[7..], xirius_prefs::Os::Win)
                    .is_ok_and(|b| b != 0 && crate::input_dispatch::bits(raw) & b == b)
        });
        if let Some(action) = action {
            match action {
                "note.resize --equal" => {
                    out.ctrl = true;
                    out.alt = false;
                }
                "note.resize --stretch" => {
                    out.ctrl = false;
                    out.alt = true;
                }
                "note.resize --align" | "note.strum" => {
                    out.ctrl = true;
                    out.alt = true;
                }
                _ => {}
            }
        }
        return out;
    }
    if !map.changed.contains_key(context) {
        return raw;
    }
    match action {
        Some("note.move" | "clip.move" | "note.resize" | "clip.resize" | "clip.loop.set") => Mods {
            ctrl: false,
            alt: false,
            ..raw
        },
        Some(
            "note.duplicate"
            | "clip.duplicate"
            | "note.resize --adjacent"
            | "clip.stretch"
            | "clip.loop.set --start",
        ) => Mods {
            ctrl: false,
            alt: true,
            ..raw
        },
        Some("clip.slip" | "note.velocity.set") => Mods {
            ctrl: true,
            alt: true,
            ..raw
        },
        // The pencil draws freehand; the line is the one with Alt, and neither is a snap switch.
        Some("cc.draw") => Mods {
            shift: false,
            alt: false,
            ..raw
        },
        Some("cc.draw --line") => Mods {
            shift: false,
            alt: true,
            ..raw
        },
        Some("select.toggle" | "select.rect --add") => Mods::shift(),
        Some("select.set" | "select.rect") => Mods::NONE,
        _ => raw,
    }
}

pub fn roll_zone(zone: crate::roll::Zone) -> &'static str {
    match zone {
        crate::roll::Zone::Body => "note-body",
        crate::roll::Zone::EdgeL | crate::roll::Zone::EdgeR => "note-edge",
        crate::roll::Zone::Velocity => "note-upper",
        crate::roll::Zone::Split => "note-lower",
    }
}

/// The part of a controller lane a press lands on: the bar or the point under the pointer, or
/// the empty part of the lane.
pub fn lane_zone(velocity: bool, on_target: bool) -> &'static str {
    match (on_target, velocity) {
        (false, _) => "empty",
        (true, true) => "velocity-bar",
        (true, false) => "cc-point",
    }
}

/// The lane's tool: the pencil and the line are the draw tool's, the rest select.
pub fn lane_tool(tool: crate::tools::Tool) -> &'static str {
    match tool {
        crate::tools::Tool::Draw => "draw",
        _ => "select",
    }
}

/// An automation lane has the select tool's gestures, and the erase tool's for a point.
pub fn automation_tool(tool: crate::tools::Tool) -> &'static str {
    match tool {
        crate::tools::Tool::Eraser => "erase",
        _ => "select",
    }
}

pub fn automation_zone(spot: &crate::arrange::Spot) -> Option<&'static str> {
    use crate::arrange::Spot;
    match spot {
        Spot::Point { .. } => Some("point"),
        Spot::Line { .. } => Some("line"),
        Spot::Lane { .. } => Some("empty"),
        _ => None,
    }
}

/// Where each action the map can bind is read (KEY-016). Every entry of
/// [`xirius_prefs::mousemap::ACTIONS`] has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Sets the modifiers a drag starts with ([`modifiers`]).
    Start,
    /// A modifier pressed in the middle of the drag marks it ([`during`]).
    During,
    /// The click's own work (select, mute, cut, move the play position, delete a point).
    Click,
    /// Picks what the drag does (move, resize, rectangle, velocity, bend, add a point).
    Mode,
    /// A turn of the wheel, or the ruler's drag up and down.
    Wheel,
}

pub const ROUTES: &[(&str, Route)] = &[
    ("select.set", Route::Click),
    ("select.toggle", Route::Click),
    ("select.none", Route::Click),
    ("select.after", Route::Click),
    ("select.rect", Route::Mode),
    ("select.rect --add", Route::Mode),
    ("note.move", Route::Mode),
    ("note.duplicate", Route::Mode),
    ("note.velocity.set", Route::Mode),
    ("note.resize", Route::Mode),
    ("note.resize --adjacent", Route::Mode),
    ("note.resize --equal", Route::During),
    ("note.resize --stretch", Route::During),
    ("note.resize --align", Route::During),
    ("note.strum", Route::During),
    ("note.mute", Route::Click),
    ("note.split", Route::Click),
    ("note.split --deep", Route::Click),
    ("note.audition", Route::Mode),
    ("snap.invert", Route::During),
    ("clip.move", Route::Mode),
    ("clip.duplicate", Route::Mode),
    ("clip.slip", Route::Mode),
    ("clip.resize", Route::Mode),
    ("clip.stretch", Route::Mode),
    ("clip.fade", Route::Mode),
    ("clip.mute", Route::Click),
    ("transport.locate", Route::Click),
    ("cc.draw", Route::Start),
    ("cc.draw --line", Route::Start),
    ("cc.point.move", Route::Mode),
    ("cc.point.move --lock", Route::During),
    ("auto.point.add", Route::Mode),
    ("auto.point.move", Route::Mode),
    ("auto.point.move --lock", Route::During),
    ("auto.point.delete", Route::Click),
    ("auto.segment.shape", Route::Mode),
    ("view.zoom.time", Route::Wheel),
    ("view.zoom.data", Route::Wheel),
    ("view.scroll", Route::Wheel),
    ("clip.loop.set", Route::Mode),
    ("clip.loop.set --start", Route::Mode),
];

pub fn route(action: &str) -> Option<Route> {
    ROUTES.iter().find(|(a, _)| *a == action).map(|(_, r)| *r)
}

pub fn arrange_zone(spot: &crate::arrange::Spot) -> Option<&'static str> {
    use crate::arrange::Spot;
    match spot {
        Spot::Clip {
            grip: crate::arrange::Grip::Body,
            ..
        } => Some("clip-body"),
        Spot::Clip {
            grip: crate::arrange::Grip::Corner,
            ..
        } => Some("clip-corner"),
        Spot::Clip { .. } => Some("clip-edge"),
        Spot::Fx {
            part:
                crate::arrange::clipfx::FxPart::Flag(_)
                | crate::arrange::clipfx::FxPart::Mid(_)
                | crate::arrange::clipfx::FxPart::Cross,
            ..
        } => Some("fade-handle"),
        Spot::Space(_) => Some("empty"),
        _ => None,
    }
}
