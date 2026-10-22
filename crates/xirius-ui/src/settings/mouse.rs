//! ショートカット, マウス: what the select tool's clicks and drags do on each part of a note
//! or a clip, by the modifier held, and the tools a held modifier switches to (KEY-018〜020).

use xirius_design::{FontSize, Radius, Weight};
use xirius_prefs::Settings;
use xirius_prefs::chord::Chord;
use xirius_prefs::mousemap::{self, AREAS, MOD_COLUMNS, Named, TOOLS};

use super::parts::{self, Seg};
use super::scene::{self, B};
use super::{Do, MenuKind, SettingsUi};
use crate::draw::Align;
use crate::studio::Rect;

/// The part a press on the sample picks first.
pub fn first_zone(area: &str) -> &'static str {
    mousemap::zones(area).first().map(|z| z.name).unwrap_or("")
}

/// The tools the table is written for: the selecting tool's in the roll and the arrangement
/// (the others only hold), the two a lane tells apart, and `any` on the ruler.
pub fn table_tools(area: &str) -> Vec<Named> {
    match area {
        "roll" | "arrange" => mousemap::tools(area)
            .into_iter()
            .filter(|t| t.name == "select")
            .collect(),
        _ => mousemap::tools(area),
    }
}

/// What a cell can be set to: nothing, or an action of its area.
pub fn choices(ctx: &str) -> Vec<(String, String)> {
    let area = ctx.split('.').next().unwrap_or("");
    // The ruler's wheel and its drag move the view; its click moves the play position.
    let view = ctx.ends_with(".wheel") || ctx.ends_with(".drag");
    let mut out = vec![(String::new(), "—".to_string())];
    out.extend(
        mousemap::ACTIONS
            .iter()
            .filter(|(a, _)| {
                mousemap::allowed_in(area, a)
                    && match (area, view) {
                        ("ruler", true) => a.starts_with("view."),
                        ("ruler", false) => *a == "transport.locate",
                        _ => true,
                    }
            })
            .map(|(a, l)| (a.to_string(), l.to_string())),
    );
    out
}

/// A modifier as the keyboard says it (`Mod+Alt` → `Ctrl+Alt`, `none` → `なし`).
pub fn mods_label(m: &str) -> String {
    if m == "none" || m.is_empty() {
        return "なし".to_string();
    }
    m.split('+')
        .map(|p| if p == "Mod" { "Ctrl" } else { p })
        .collect::<Vec<_>>()
        .join("+")
}

fn tool_icon(tool: &str) -> &'static str {
    match tool {
        "select" => "select",
        "range" => "range",
        "cut" => "cut",
        "erase" => "eraser",
        "draw" => "pencil",
        "mute" => "mute",
        _ => "listen",
    }
}

fn tool_label(tool: &str) -> &'static str {
    TOOLS
        .iter()
        .find(|t| t.name == tool)
        .map(|t| t.label)
        .unwrap_or("")
}

fn zone_label(area: &str, zone: &str) -> &'static str {
    mousemap::zones(area)
        .iter()
        .find(|z| z.name == zone)
        .map(|z| z.label)
        .unwrap_or("")
}

/// The modifiers held, as the map writes them (`None` when none are).
fn held_text(ui: &SettingsUi) -> Option<String> {
    (ui.held != 0).then(|| Chord::mods_text(ui.held))
}

pub fn mouse(b: &mut B, ui: &SettingsUi, s: &Settings, x: f32, top: f32, w: f32) -> f32 {
    let c = b.c;
    let area = ui.area.as_str();
    let mut y = top + 4.0;
    // Which area, and the tool the table is for.
    let segs: Vec<Seg> = AREAS
        .iter()
        .map(|a| Seg {
            label: a.label,
            icon: None,
            act: Do::Area(a.name.to_string()),
            on: area == a.name,
            tip: None,
        })
        .collect();
    let sr = parts::seg_with(b, x, y, &segs, 24.0, 2.0, 30.0, 8.0);
    let tools = table_tools(area);
    let mut bottom = sr.bottom();
    let tool = tools
        .iter()
        .find(|t| t.name == ui.tool)
        .or(tools.first())
        .map_or("select", |t| t.name);
    // The tool the table is for: a tag when there is one, a choice of two for the lanes. It
    // goes under the areas when it does not fit beside them.
    if tools.len() == 1 {
        let tag = match tool {
            "select" => "選択ツール",
            _ => tools[0].label,
        };
        let tw = 8.0 + 14.0 + 4.0 + scene::width(tag, FontSize::Caption) + 8.0;
        let mut tr = Rect::new(sr.right() + 12.0, sr.y + (sr.h - 20.0) / 2.0, tw, 20.0);
        if tr.right() > x + w {
            tr = Rect::new(x, sr.bottom() + 8.0, tw, 20.0);
            bottom = tr.bottom();
        }
        b.round(tr, c.surface_3, Radius::Pill);
        if tool == "select" {
            b.icon("select", parts::small_icon_at(tr.x + 8.0, tr), c.ink);
        }
        b.text(
            tag,
            FontSize::Caption,
            Weight::Regular,
            c.ink_strong,
            Rect::new(tr.x + 8.0 + 14.0 + 4.0, tr.y, tw, tr.h),
            Align::Left,
        );
    } else {
        let segs: Vec<Seg> = tools
            .iter()
            .map(|t| Seg {
                label: t.label,
                icon: Some(tool_icon(t.name)),
                act: Do::MouseTool(t.name.to_string()),
                on: tool == t.name,
                tip: None,
            })
            .collect();
        // Two tools do not fit beside five areas: they go under them.
        let under = parts::seg_with(b, x, sr.bottom() + 8.0, &segs, 24.0, 2.0, 30.0, 8.0);
        bottom = under.bottom();
    }
    y = bottom + 10.0;
    // The sample: a press on a part shows what it does.
    let smp = Rect::new(x, y, w, 150.0);
    sample(b, ui, area, smp);
    y = smp.bottom() + 14.0;
    let zl = zone_label(area, &ui.zone);
    let zw = b.text(
        zl,
        FontSize::Body,
        Weight::Semibold,
        c.ink_strong,
        Rect::new(x, y, w, 20.0),
        Align::Left,
    );
    // The internal name of the table is for the tooltip only.
    b.tip(
        Rect::new(x, y, zw.max(40.0), 20.0),
        &format!("{area}.{tool}.{}", ui.zone),
    );
    y += 20.0 + 6.0;
    y = table(b, ui, s, area, tool, x, y, w);
    if !mousemap::has_holds(area) {
        return y;
    }
    y = parts::gh(b, "一時的なツールの切り替え", x, y);
    holds(b, ui, s, area, x, y, w)
}

/// A part of the sample: lit under the pointer, and more when it is the one shown.
fn zone(b: &mut B, ui: &SettingsUi, r: Rect, name: &str, label: &str, radius: Radius) {
    let c = b.c;
    let on = ui.zone == name;
    if on {
        b.round(r, c.sel, radius);
    } else if b.hot(r) {
        b.round(r, c.hover, radius);
    }
    b.item(r, Do::Zone(name.to_string()), Some(label.to_string()));
}

/// The empty part of the sample, under the rest.
fn background(b: &mut B, ui: &SettingsUi, smp: Rect, label: &str) {
    let c = b.c;
    if ui.zone == "empty" {
        b.round(smp, c.surface_3, Radius::M);
    }
    b.item(smp, Do::Zone("empty".to_string()), Some(label.to_string()));
}

fn sample(b: &mut B, ui: &SettingsUi, area: &str, smp: Rect) {
    let c = b.c;
    let fc = super::faces(c);
    b.round(smp, fc.well, Radius::M);
    b.push_mask(smp);
    let at = |f: f32| smp.x + smp.w * f;
    if area == "roll" {
        background(b, ui, smp, zone_label(area, "empty"));
        let mut faint = c.tracks[0];
        faint.a *= 0.7;
        for (l, t, nw) in [(0.06, 96.0, 0.12), (0.62, 30.0, 0.16), (0.80, 96.0, 0.10)] {
            b.round(
                Rect::new(at(l), smp.y + t, smp.w * nw, 26.0),
                faint,
                Radius::Xs,
            );
        }
        let n = Rect::new(at(0.24), smp.y + 46.0, smp.w * 0.32, 48.0);
        b.round(n, c.tracks[0], Radius::Xs);
        let lab = |z| zone_label(area, z);
        zone(b, ui, n, "note-body", lab("note-body"), Radius::Xs);
        zone(
            b,
            ui,
            Rect::new(n.x + 10.0, n.y, n.w - 20.0, 14.0),
            "note-upper",
            lab("note-upper"),
            Radius::Xs,
        );
        zone(
            b,
            ui,
            Rect::new(n.x + 10.0, n.bottom() - 14.0, n.w - 20.0, 14.0),
            "note-lower",
            lab("note-lower"),
            Radius::Xs,
        );
        for ex in [n.x, n.right() - 10.0] {
            zone(
                b,
                ui,
                Rect::new(ex, n.y, 10.0, n.h),
                "note-edge",
                lab("note-edge"),
                Radius::Xs,
            );
        }
    } else if area == "lane" {
        lane_sample(b, ui, area, smp);
    } else if area == "automation" {
        automation_sample(b, ui, area, smp);
    } else if area == "ruler" {
        ruler_sample(b, ui, area, smp);
    } else {
        background(b, ui, smp, zone_label(area, "empty"));
        let clips = [
            (Rect::new(at(0.22), smp.y + 44.0, smp.w * 0.40, 76.0), 300.0),
            (Rect::new(at(0.66), smp.y + 44.0, smp.w * 0.22, 76.0), 160.0),
        ];
        for (r, view) in clips {
            b.round(r, c.clip, Radius::Clip);
            wave(b, r, view);
        }
        let k = clips[0].0;
        let lab = |z| zone_label(area, z);
        zone(
            b,
            ui,
            Rect::new(k.x + 10.0, k.y + 16.0, k.w - 20.0, k.h - 16.0),
            "clip-body",
            lab("clip-body"),
            Radius::Xs,
        );
        zone(
            b,
            ui,
            Rect::new(k.x + 26.0, k.y, k.w - 52.0, 16.0),
            "clip-body",
            lab("clip-body"),
            Radius::Xs,
        );
        for fx in [k.x + 10.0, k.right() - 10.0 - 16.0] {
            zone(
                b,
                ui,
                Rect::new(fx, k.y, 16.0, 16.0),
                "fade-handle",
                lab("fade-handle"),
                Radius::Xs,
            );
        }
        for ex in [k.x, k.right() - 10.0] {
            zone(
                b,
                ui,
                Rect::new(ex, k.y, 10.0, k.h),
                "clip-edge",
                lab("clip-edge"),
                Radius::Xs,
            );
        }
        zone(
            b,
            ui,
            Rect::new(k.right() - 10.0, k.bottom() - 12.0, 10.0, 12.0),
            "clip-corner",
            lab("clip-corner"),
            Radius::Xs,
        );
    }
    b.pop_mask();
}

/// A line of dots from one point to the other (there is no stroke to draw it with).
fn dots(b: &mut B, from: (f32, f32), to: (f32, f32), color: gpui::Hsla) {
    let n = ((to.0 - from.0).abs().max((to.1 - from.1).abs()) / 2.0)
        .ceil()
        .max(1.0) as usize;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let (px, py) = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        b.fill(Rect::new(px - 1.0, py - 1.0, 2.0, 2.0), color);
    }
}

/// The controller lanes: velocity bars over a controller's curve.
fn lane_sample(b: &mut B, ui: &SettingsUi, area: &str, smp: Rect) {
    let c = b.c;
    let at = |f: f32| smp.x + smp.w * f;
    background(b, ui, smp, zone_label(area, "empty"));
    let base = smp.y + 78.0;
    for (i, h) in [34.0, 52.0, 24.0, 60.0, 42.0, 30.0, 56.0, 38.0]
        .iter()
        .enumerate()
    {
        let r = Rect::new(at(0.06 + i as f32 * 0.075), base - h, 12.0, *h);
        b.round(r, c.tracks[0], Radius::Xs);
        zone(
            b,
            ui,
            r,
            "velocity-bar",
            zone_label(area, "velocity-bar"),
            Radius::Xs,
        );
    }
    b.fill(Rect::new(smp.x, base + 3.0, smp.w, 1.0), c.surface_3);
    let pts = [(0.10, 138.0), (0.32, 100.0), (0.55, 124.0), (0.80, 92.0)];
    let at_pt = |p: &(f32, f32)| (at(p.0), smp.y + p.1);
    for w in pts.windows(2) {
        dots(b, at_pt(&w[0]), at_pt(&w[1]), c.tracks[1]);
    }
    for p in &pts {
        let (px, py) = at_pt(p);
        let r = Rect::new(px - 5.0, py - 5.0, 10.0, 10.0);
        b.round(r, c.tracks[1], Radius::Pill);
        zone(
            b,
            ui,
            r,
            "cc-point",
            zone_label(area, "cc-point"),
            Radius::Pill,
        );
    }
}

/// An automation lane: a line through its points.
fn automation_sample(b: &mut B, ui: &SettingsUi, area: &str, smp: Rect) {
    let c = b.c;
    let at = |f: f32| smp.x + smp.w * f;
    background(b, ui, smp, zone_label(area, "empty"));
    let pts = [
        (0.08, 110.0),
        (0.30, 50.0),
        (0.52, 50.0),
        (0.74, 96.0),
        (0.92, 36.0),
    ];
    let at_pt = |p: &(f32, f32)| (at(p.0), smp.y + p.1);
    for w in pts.windows(2) {
        let (a, z) = (at_pt(&w[0]), at_pt(&w[1]));
        dots(b, a, z, c.tracks[1]);
        let mid = ((a.0 + z.0) / 2.0, (a.1 + z.1) / 2.0);
        zone(
            b,
            ui,
            Rect::new(mid.0 - 14.0, mid.1 - 6.0, 28.0, 12.0),
            "line",
            zone_label(area, "line"),
            Radius::Xs,
        );
    }
    for p in &pts {
        let (px, py) = at_pt(p);
        let r = Rect::new(px - 5.0, py - 5.0, 10.0, 10.0);
        b.round(r, c.tracks[1], Radius::Pill);
        zone(b, ui, r, "point", zone_label(area, "point"), Radius::Pill);
    }
}

/// The ruler over the tracks: the whole strip is the one part.
fn ruler_sample(b: &mut B, ui: &SettingsUi, area: &str, smp: Rect) {
    let c = b.c;
    let strip = Rect::new(smp.x, smp.y, smp.w, 28.0);
    b.fill(strip, c.surface_3);
    for i in 0..8 {
        let tx = smp.x + 16.0 + i as f32 * (smp.w - 32.0) / 8.0;
        b.fill(Rect::new(tx, smp.y + 16.0, 1.0, 12.0), c.ink);
        b.text(
            &(i + 1).to_string(),
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(tx + 4.0, smp.y + 4.0, 24.0, 16.0),
            Align::Left,
        );
    }
    zone(b, ui, strip, "empty", zone_label(area, "empty"), Radius::Xs);
    let mut faint = c.tracks[0];
    faint.a *= 0.5;
    for (i, (l, nw)) in [(0.10, 0.30), (0.45, 0.20), (0.20, 0.34)]
        .iter()
        .enumerate()
    {
        b.round(
            Rect::new(
                smp.x + smp.w * l,
                smp.y + 44.0 + i as f32 * 34.0,
                smp.w * nw,
                24.0,
            ),
            faint,
            Radius::Xs,
        );
    }
}

/// The bars of a clip's wave, drawn in a view `view` wide and 76 high.
fn wave(b: &mut B, r: Rect, view: f32) {
    let c = b.c;
    let sx = r.w / view;
    let sy = r.h / 76.0;
    for i in 0..60 {
        let fx = i as f32 * 5.0 + 2.0;
        if fx + 3.0 > view {
            break;
        }
        let f = i as f32;
        let h = 6.0 + 26.0 * ((f * 0.7).sin() * (f * 0.23).cos()).abs();
        b.round(
            Rect::new(r.x + fx * sx, r.y + (38.0 - h / 2.0) * sy, 3.0 * sx, h * sy),
            c.tracks[1],
            Radius::Pill,
        );
    }
}

/// What each modifier does for a click, a drag, and a key pressed during a drag.
#[allow(clippy::too_many_arguments)]
fn table(
    b: &mut B,
    ui: &SettingsUi,
    s: &Settings,
    area: &str,
    tool: &str,
    x: f32,
    y: f32,
    w: f32,
) -> f32 {
    let c = b.c;
    let first = 118.0;
    let gap = 2.0;
    let n = MOD_COLUMNS.len() as f32;
    let cw = (w - first - gap * n) / n;
    let col = |i: usize| x + first + gap + (cw + gap) * i as f32;
    let held = held_text(ui);
    let holds = if mousemap::has_holds(area) {
        s.mousemap.table(&format!("{area}.select.hold"))
    } else {
        Default::default()
    };
    let wheel = area == "ruler";
    let rows: &[(&str, &str, bool)] = if wheel {
        &[
            ("クリック", "click", false),
            ("縦ドラッグ", "drag", false),
            ("ホイール", "wheel", false),
        ]
    } else {
        &[
            ("クリック", "click", false),
            ("ドラッグ", "drag", false),
            ("ドラッグ中に押す", "drag", true),
        ]
    };
    // The column of the modifiers held.
    if let Some(h) = &held
        && let Some(i) = MOD_COLUMNS.iter().position(|m| m == h)
    {
        b.round(
            Rect::new(
                col(i) - 1.0,
                y,
                cw + 2.0,
                26.0 + (26.0 + gap) * rows.len() as f32 + 2.0,
            ),
            c.sel,
            Radius::S,
        );
    }
    for (i, m) in MOD_COLUMNS.iter().enumerate() {
        b.text(
            &mods_label(m),
            FontSize::Caption,
            Weight::Semibold,
            c.ink,
            Rect::new(col(i), y, cw, 26.0),
            Align::Center,
        );
    }
    let mut ry = y + 26.0 + gap;
    for (label, gesture, during) in rows.iter().copied() {
        b.text(
            label,
            FontSize::Small,
            Weight::Regular,
            c.ink_strong,
            Rect::new(x + 4.0, ry, first - 4.0, 26.0),
            Align::Left,
        );
        let ctx = format!("{area}.{tool}.{}.{gesture}", ui.zone);
        for (i, m) in MOD_COLUMNS.iter().enumerate() {
            let r = Rect::new(col(i), ry, cw, 26.0);
            if during && *m == "none" {
                continue;
            }
            let modifier = if during {
                format!("during.{m}")
            } else {
                m.to_string()
            };
            if !during && let Some(to) = holds.get(*m) {
                let t = to.strip_prefix("tool:").unwrap_or(to);
                // In blue, cut with 「…」 when the column is narrow; the tooltip has it all.
                let full = format!("{}（一時的）", tool_label(t));
                b.field(r, "", false);
                let tr = Rect::new(r.x + 8.0, r.y, r.w - 8.0 - 24.0, r.h);
                let shown = super::fit_back(&full, FontSize::Caption, tr.w);
                b.text(
                    &shown,
                    FontSize::Caption,
                    Weight::Regular,
                    c.ink_blue,
                    tr,
                    Align::Left,
                );
                if shown != full {
                    b.tip(r, &full);
                }
                continue;
            }
            let a = s.mousemap.get(&ctx, &modifier);
            let shown = if a.is_empty() {
                "—"
            } else {
                mousemap::action_label(&a).unwrap_or("—")
            };
            b.field(r, shown, true);
            b.item(
                r,
                Do::Menu(MenuKind::MouseCell {
                    ctx: ctx.clone(),
                    modifier: modifier.clone(),
                }),
                None,
            );
            let changed = s
                .mousemap
                .changed
                .get(&ctx)
                .is_some_and(|t| t.contains_key(&modifier));
            if changed {
                b.round(
                    Rect::new(r.right() - 8.0, r.y + 2.0, 6.0, 6.0),
                    c.ink_blue,
                    Radius::Pill,
                );
                b.tip(Rect::new(r.right() - 10.0, r.y, 10.0, 10.0), "変更済み");
            }
        }
        ry += 26.0 + gap;
    }
    ry
}

/// The modifier a new hold of `tool` takes (the first one it does not use) and where it
/// goes: (context, modifier, action).
pub fn first_free_hold(s: &Settings, area: &str, tool: &str) -> Option<(String, String, String)> {
    let ctx = format!("{area}.{tool}.hold");
    let m = mousemap::HOLD_MODS
        .iter()
        .find(|m| s.mousemap.get(&ctx, m).is_empty())?;
    let def = mousemap::default_of(&ctx, m);
    let to = if !def.is_empty() {
        def.to_string()
    } else if tool == "cut" {
        "tool:select".to_string()
    } else {
        "tool:cut".to_string()
    };
    Some((ctx, m.to_string(), to))
}

/// The tools a hold can still be added for.
pub fn addable_tools(s: &Settings, area: &str) -> Vec<Named> {
    TOOLS
        .iter()
        .filter(|t| first_free_hold(s, area, t.name).is_some())
        .copied()
        .collect()
}

/// Says what a hold hides: the click or drag on the same modifier of the same tool.
fn hides(area: &str, tool: &str, modifier: &str, s: &Settings) -> Option<String> {
    let prefix = format!("{area}.{tool}.");
    let (ctx, _, _) = s
        .mousemap
        .warnings()
        .into_iter()
        .find(|(c, m, _)| c.starts_with(&prefix) && m == modifier)?;
    let p: Vec<&str> = ctx.split('.').collect();
    let gesture = mousemap::GESTURES
        .iter()
        .find(|g| Some(&g.name) == p.get(3))
        .map_or("", |g| g.label);
    let did = s.mousemap.get(&ctx, modifier);
    Some(format!(
        "{} の{gesture}（{}）と同じ修飾キーです。ツールの切り替えが優先されます",
        zone_label(area, p.get(2).copied().unwrap_or("")),
        mousemap::action_label(&did).unwrap_or("—"),
    ))
}

/// The tools a held modifier switches to, one row per tool and modifier.
fn holds(b: &mut B, ui: &SettingsUi, s: &Settings, area: &str, x: f32, top: f32, w: f32) -> f32 {
    let c = b.c;
    let held = held_text(ui);
    let mut y = top;
    for tool in TOOLS {
        let ctx = format!("{area}.{}.hold", tool.name);
        for (m, to) in s.mousemap.table(&ctx) {
            let row = Rect::new(x, y, w, 40.0);
            if held.as_deref() == Some(m.as_str()) {
                b.round(row, c.sel, Radius::S);
            } else if b.hot(row) {
                b.round(row, c.hover, Radius::S);
            }
            // The tool.
            let tw = scene::width(tool.label, FontSize::Caption);
            let chip = Rect::new(row.x + 6.0, row.y + 8.0, 6.0 + 14.0 + 6.0 + tw + 8.0, 24.0);
            b.round(chip, c.surface_3, Radius::S);
            b.icon(
                tool_icon(tool.name),
                parts::small_icon_at(chip.x + 6.0, chip),
                c.ink,
            );
            b.text(
                tool.label,
                FontSize::Caption,
                Weight::Regular,
                c.ink_strong,
                Rect::new(chip.x + 6.0 + 14.0 + 6.0, chip.y, tw + 2.0, chip.h),
                Align::Left,
            );
            // From the end: back to the start (or away, for a row that was added), the tool,
            // the arrow, the modifier.
            let rs = Rect::new(row.right() - 4.0 - 26.0 + 2.0, row.y + 9.0, 22.0, 22.0);
            let default = mousemap::default_of(&ctx, &m);
            if default.is_empty() {
                parts::icon_button(
                    b,
                    rs,
                    "trash",
                    Do::HoldRemove(ctx.clone(), m.clone()),
                    Some("削除"),
                );
            } else if to != default {
                parts::icon_button(
                    b,
                    rs,
                    "restore",
                    Do::HoldRestore(ctx.clone(), m.clone()),
                    Some("元に戻す"),
                );
            }
            let to_r = Rect::new(rs.x - 2.0 - 8.0 - 150.0, row.y + 7.0, 150.0, 26.0);
            let t = to.strip_prefix("tool:").unwrap_or(&to);
            b.field(to_r, tool_label(t), true);
            b.item(
                to_r,
                Do::Menu(MenuKind::HoldTo(ctx.clone(), m.clone())),
                None,
            );
            let arrow = Rect::new(to_r.x - 8.0 - 16.0, row.y, 16.0, row.h);
            b.icon(
                "chevron-right",
                parts::small_icon_at(arrow.x + 1.0, arrow),
                c.ink,
            );
            let mr = Rect::new(arrow.x - 8.0 - 96.0, row.y + 7.0, 96.0, 26.0);
            b.field(mr, &mods_label(&m), true);
            b.item(
                mr,
                Do::Menu(MenuKind::HoldMod(ctx.clone(), m.clone())),
                None,
            );
            if let Some(text) = hides(area, tool.name, &m, s) {
                let wr = Rect::new(mr.x - 8.0 - 14.0, row.y + 13.0, 14.0, 14.0);
                b.icon("layers", wr, c.ink);
                b.tip(wr, &text);
            }
            y = row.bottom() + 2.0;
            // A modifier the tool already has: the row that was refused says so.
            if let Some((ectx, em, text)) = &ui.hold_error
                && *ectx == ctx
                && *em == m
            {
                b.icon(
                    "alert",
                    parts::small_icon_at(row.x + 6.0, Rect::new(row.x, y, 20.0, 20.0)),
                    c.alert,
                );
                b.text(
                    text,
                    FontSize::Caption,
                    Weight::Regular,
                    c.ink_alert,
                    Rect::new(row.x + 6.0 + 14.0 + 6.0, y, row.w - 30.0, 20.0),
                    Align::Left,
                );
                y += 22.0;
            }
        }
    }
    let label = "ツールを追加";
    let add = Rect::new(x, y + 6.0, parts::button_w(label), 28.0);
    parts::button(
        b,
        add,
        "plus",
        label,
        Do::Menu(MenuKind::HoldAdd(area.to_string())),
    );
    add.bottom() + 8.0
}
