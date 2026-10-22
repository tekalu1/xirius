//! The mousemap: what a click or a drag does with each modifier, and which tool a held
//! modifier switches to (06 §3.3, KEY-019, KEY-026).
//!
//! A context is `<area>.<tool>.<zone>.<gesture>` (`roll.select.note-body.drag`) or
//! `<area>.<tool>.hold` (the piano roll and the arrangement only). The controller lanes
//! (`lane.select.velocity-bar.drag`), the automation lanes (`automation.select.point.drag`) and the
//! ruler (`ruler.any.empty.wheel`, whose tool is `any`: every tool acts alike there) name their
//! hit areas the same way (KEY-016, KEY-017). In a gesture's context a modifier is `none`, `Shift`, `Mod+Alt`, ...,
//! or `during.Shift` for a key pressed in the middle of a drag; in a hold's context it is the
//! modifier held and the value is `tool:<name>`. Only what the user changed is kept, `""` for
//! "nothing".

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::chord::{Chord, Os};
use crate::tomlw;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Named {
    pub name: &'static str,
    pub label: &'static str,
}

const fn n(name: &'static str, label: &'static str) -> Named {
    Named { name, label }
}

pub const AREAS: &[Named] = &[
    n("roll", "ピアノロール"),
    n("arrange", "アレンジ"),
    n("lane", "コントローラー"),
    n("automation", "オートメーション"),
    n("ruler", "ルーラー"),
];

/// The areas that have tool holds (the others read the tool as it is).
pub fn has_holds(area: &str) -> bool {
    matches!(area, "roll" | "arrange")
}

/// The tools a context of `area` is written for: the toolbar's for the roll and the arrangement,
/// the two that change what a lane does for the controller lanes, the select and erase tools on
/// automation lanes, and `any` on the ruler.
pub fn tools(area: &str) -> Vec<Named> {
    let pick = |names: &[&str]| -> Vec<Named> {
        TOOLS
            .iter()
            .filter(|t| names.contains(&t.name))
            .copied()
            .collect()
    };
    match area {
        "roll" | "arrange" => TOOLS.to_vec(),
        "lane" => pick(&["select", "draw"]),
        "automation" => pick(&["select", "erase"]),
        "ruler" => vec![n("any", "すべてのツール")],
        _ => Vec::new(),
    }
}

/// The tools, in the toolbar's order.
pub const TOOLS: &[Named] = &[
    n("select", "選択"),
    n("range", "範囲"),
    n("cut", "分割"),
    n("erase", "消しゴム"),
    n("draw", "鉛筆"),
    n("mute", "ミュート"),
    n("listen", "試聴"),
];

pub const GESTURES: &[Named] = &[
    n("click", "クリック"),
    n("drag", "ドラッグ"),
    n("wheel", "ホイール"),
];

/// The gestures `area` has: a click and a drag, and on the ruler a click, a drag (up and down)
/// and the wheel.
pub fn gestures(area: &str) -> Vec<Named> {
    let keep: &[&str] = if area == "ruler" {
        &["click", "drag", "wheel"]
    } else {
        &["click", "drag"]
    };
    GESTURES
        .iter()
        .filter(|g| keep.contains(&g.name))
        .copied()
        .collect()
}

/// The columns of a gesture's table.
pub const MOD_COLUMNS: &[&str] = &["none", "Shift", "Mod", "Mod+Shift", "Alt", "Mod+Alt"];

/// The modifiers a hold can be on.
pub const HOLD_MODS: &[&str] = &["Mod", "Alt", "Shift", "Mod+Shift", "Mod+Alt", "Alt+Shift"];

const ROLL_ZONES: &[Named] = &[
    n("note-body", "ノート"),
    n("note-edge", "ノートの端"),
    n("note-upper", "ノートの上側"),
    n("note-lower", "ノートの下側"),
    n("empty", "空白部分"),
];

const ARRANGE_ZONES: &[Named] = &[
    n("clip-body", "クリップ"),
    n("clip-edge", "クリップの端"),
    n("fade-handle", "フェードのハンドル"),
    n("empty", "空白部分"),
    n("clip-corner", "クリップの右下の角"),
];

const LANE_ZONES: &[Named] = &[
    n("velocity-bar", "ベロシティのバー"),
    n("cc-point", "コントローラーの点"),
    n("empty", "空白部分"),
];

const AUTOMATION_ZONES: &[Named] = &[
    n("point", "オートメーションの点"),
    n("line", "オートメーションの線"),
    n("empty", "空白部分"),
];

const RULER_ZONES: &[Named] = &[n("empty", "ルーラー")];

/// The parts of an area a click lands on.
pub fn zones(area: &str) -> &'static [Named] {
    match area {
        "roll" => ROLL_ZONES,
        "arrange" => ARRANGE_ZONES,
        "lane" => LANE_ZONES,
        "automation" => AUTOMATION_ZONES,
        "ruler" => RULER_ZONES,
        _ => &[],
    }
}

/// What a gesture can do, with the name the screen shows.
pub const ACTIONS: &[(&str, &str)] = &[
    ("select.set", "選択"),
    ("select.toggle", "選択を切り替え"),
    ("select.none", "選択を解除"),
    ("select.rect", "囲んで選択"),
    ("select.rect --add", "囲んで選択に追加"),
    ("select.after", "ここから後ろを選択"),
    ("note.move", "移動"),
    ("note.duplicate", "複製"),
    ("note.velocity.set", "ベロシティ"),
    ("note.resize", "長さ"),
    ("note.resize --adjacent", "隣接ノートも変更"),
    ("note.resize --equal", "長さをそろえる"),
    ("note.resize --stretch", "伸縮"),
    ("note.resize --align", "端をそろえる"),
    ("note.strum", "ストラム"),
    ("note.mute", "ミュート"),
    ("note.split", "分割"),
    ("note.split --deep", "重なったノートも分割"),
    ("note.audition", "試聴"),
    ("snap.invert", "スナップを反転"),
    ("clip.move", "移動"),
    ("clip.duplicate", "複製"),
    ("clip.slip", "スリップ編集"),
    ("clip.resize", "長さ"),
    ("clip.stretch", "伸縮"),
    ("clip.fade", "フェード"),
    ("clip.mute", "ミュート"),
    ("transport.locate", "再生位置を移動"),
    ("cc.draw", "描く"),
    ("cc.draw --line", "直線で描く"),
    ("cc.point.move", "点を移動"),
    ("cc.point.move --lock", "点を移動（時間を固定）"),
    ("auto.point.add", "点を追加"),
    ("auto.point.move", "点を移動"),
    ("auto.point.move --lock", "点を移動（縦か横に固定）"),
    ("auto.point.delete", "点を削除"),
    ("auto.segment.shape", "線を曲げる"),
    ("view.zoom.time", "横にズーム"),
    ("view.zoom.data", "縦にズーム"),
    ("view.scroll", "横にスクロール"),
    ("clip.loop.set", "ループの長さ"),
    ("clip.loop.set --start", "ループの頭"),
];

/// Whether `action` may be bound in `area`: the note and clip actions to the roll and the
/// arrangement, the controller lane's own to `lane`, and so on (KEY-016).
pub fn allowed_in(area: &str, action: &str) -> bool {
    // What lies after the pointer: the roll's notes and the arrangement's clips.
    if action == "select.after" {
        return matches!(area, "roll" | "arrange");
    }
    let verb = action.split_whitespace().next().unwrap_or("");
    let family = verb.split('.').next().unwrap_or("");
    match area {
        "roll" => matches!(family, "select" | "snap" | "note") || verb == "transport.locate",
        "arrange" => matches!(family, "select" | "snap" | "clip") || verb == "transport.locate",
        "lane" => matches!(family, "select" | "snap" | "cc") || verb == "note.velocity.set",
        "automation" => matches!(family, "select" | "snap" | "auto"),
        "ruler" => matches!(family, "view") || verb == "transport.locate",
        _ => false,
    }
}

pub fn action_label(action: &str) -> Option<&'static str> {
    if let Some(t) = action.strip_prefix("tool:") {
        return TOOLS.iter().find(|x| x.name == t).map(|x| x.label);
    }
    ACTIONS.iter().find(|(a, _)| *a == action).map(|(_, l)| *l)
}

/// What the app does when nothing is changed: the select tool's gestures and the holds the
/// tools have today (`effective_tool`).
pub const DEFAULTS: &[(&str, &str, &str)] = &[
    // the roll
    ("roll.select.note-body.click", "none", "select.set"),
    ("roll.select.note-body.click", "Shift", "select.toggle"),
    ("roll.select.note-body.drag", "none", "note.move"),
    ("roll.select.note-body.drag", "Alt", "note.duplicate"),
    ("roll.select.note-body.drag", "Mod+Alt", "note.velocity.set"),
    ("roll.select.note-body.drag", "during.Shift", "snap.invert"),
    ("roll.select.note-body.drag", "during.Mod+Alt", "note.strum"),
    ("roll.select.note-edge.click", "none", "select.set"),
    ("roll.select.note-edge.click", "Shift", "select.toggle"),
    ("roll.select.note-edge.drag", "none", "note.resize"),
    (
        "roll.select.note-edge.drag",
        "Alt",
        "note.resize --adjacent",
    ),
    ("roll.select.note-edge.drag", "during.Shift", "snap.invert"),
    (
        "roll.select.note-edge.drag",
        "during.Mod",
        "note.resize --equal",
    ),
    (
        "roll.select.note-edge.drag",
        "during.Alt",
        "note.resize --stretch",
    ),
    (
        "roll.select.note-edge.drag",
        "during.Mod+Alt",
        "note.resize --align",
    ),
    ("roll.select.note-upper.click", "none", "select.set"),
    ("roll.select.note-upper.click", "Alt", "note.mute"),
    ("roll.select.note-upper.drag", "none", "note.velocity.set"),
    ("roll.select.note-lower.click", "none", "note.split"),
    ("roll.select.note-lower.click", "Alt", "note.split --deep"),
    ("roll.select.note-lower.drag", "none", "note.move"),
    ("roll.select.empty.click", "none", "select.none"),
    ("roll.select.empty.click", "Alt", "select.after"),
    ("roll.select.empty.drag", "none", "select.rect"),
    ("roll.select.empty.drag", "Shift", "select.rect --add"),
    ("roll.select.hold", "Mod", "tool:draw"),
    ("roll.select.hold", "Mod+Shift", "tool:cut"),
    ("roll.draw.hold", "Mod", "tool:select"),
    ("roll.cut.hold", "Mod", "tool:select"),
    ("roll.erase.hold", "Mod", "tool:select"),
    // the arrangement
    ("arrange.select.clip-body.click", "none", "select.set"),
    ("arrange.select.clip-body.click", "Shift", "select.toggle"),
    ("arrange.select.clip-body.click", "Alt", "select.after"),
    ("arrange.select.clip-body.drag", "none", "clip.move"),
    ("arrange.select.clip-body.drag", "Alt", "clip.duplicate"),
    ("arrange.select.clip-body.drag", "Mod+Alt", "clip.slip"),
    (
        "arrange.select.clip-body.drag",
        "during.Shift",
        "snap.invert",
    ),
    ("arrange.select.clip-edge.click", "none", "select.set"),
    ("arrange.select.clip-edge.drag", "none", "clip.resize"),
    ("arrange.select.clip-edge.drag", "Alt", "clip.stretch"),
    (
        "arrange.select.clip-edge.drag",
        "during.Shift",
        "snap.invert",
    ),
    ("arrange.select.clip-corner.click", "none", "select.set"),
    ("arrange.select.clip-corner.drag", "none", "clip.loop.set"),
    (
        "arrange.select.clip-corner.drag",
        "Alt",
        "clip.loop.set --start",
    ),
    (
        "arrange.select.clip-corner.drag",
        "during.Shift",
        "snap.invert",
    ),
    ("arrange.select.fade-handle.drag", "none", "clip.fade"),
    (
        "arrange.select.fade-handle.drag",
        "during.Shift",
        "snap.invert",
    ),
    ("arrange.select.empty.click", "none", "select.none"),
    ("arrange.select.empty.click", "Alt", "select.after"),
    ("arrange.select.empty.drag", "none", "select.rect"),
    ("arrange.select.empty.drag", "Shift", "select.rect --add"),
    ("arrange.select.hold", "Mod", "tool:range"),
    ("arrange.range.hold", "Alt", "tool:select"),
    ("arrange.cut.hold", "Alt", "tool:select"),
    ("arrange.erase.hold", "Alt", "tool:select"),
    ("arrange.mute.hold", "Alt", "tool:select"),
    // the controller lanes under the roll
    ("lane.select.velocity-bar.click", "none", "select.set"),
    ("lane.select.velocity-bar.click", "Shift", "select.toggle"),
    ("lane.select.velocity-bar.drag", "none", "note.velocity.set"),
    ("lane.select.cc-point.click", "none", "select.set"),
    ("lane.select.cc-point.click", "Shift", "select.toggle"),
    ("lane.select.cc-point.drag", "none", "cc.point.move"),
    (
        "lane.select.cc-point.drag",
        "during.Mod",
        "cc.point.move --lock",
    ),
    ("lane.select.cc-point.drag", "during.Shift", "snap.invert"),
    ("lane.select.empty.drag", "none", "select.rect"),
    ("lane.select.empty.drag", "Shift", "select.rect --add"),
    ("lane.draw.velocity-bar.drag", "none", "cc.draw"),
    ("lane.draw.velocity-bar.drag", "Shift", "cc.draw --line"),
    ("lane.draw.velocity-bar.drag", "Alt", "cc.draw --line"),
    ("lane.draw.cc-point.drag", "none", "cc.draw"),
    ("lane.draw.cc-point.drag", "Shift", "cc.draw --line"),
    ("lane.draw.cc-point.drag", "Alt", "cc.draw --line"),
    ("lane.draw.empty.drag", "none", "cc.draw"),
    ("lane.draw.empty.drag", "Shift", "cc.draw --line"),
    ("lane.draw.empty.drag", "Alt", "cc.draw --line"),
    // the automation lanes under the arrangement
    ("automation.select.point.click", "none", "select.set"),
    ("automation.select.point.click", "Shift", "select.toggle"),
    ("automation.select.point.click", "Alt", "auto.point.delete"),
    ("automation.select.point.drag", "none", "auto.point.move"),
    (
        "automation.select.point.drag",
        "during.Mod",
        "auto.point.move --lock",
    ),
    (
        "automation.select.point.drag",
        "during.Shift",
        "snap.invert",
    ),
    ("automation.select.line.drag", "none", "auto.segment.shape"),
    ("automation.select.empty.click", "none", "select.none"),
    ("automation.select.empty.drag", "none", "select.rect"),
    ("automation.select.empty.drag", "Shift", "select.rect --add"),
    ("automation.erase.point.click", "none", "auto.point.delete"),
    // the ruler
    ("ruler.any.empty.click", "none", "transport.locate"),
    ("ruler.any.empty.drag", "none", "view.zoom.time"),
    ("ruler.any.empty.wheel", "none", "view.zoom.time"),
];

/// The name a context had in an older file: the ruler used to be a part of the arrangement.
fn canonical(ctx: &str) -> &str {
    let p: Vec<&str> = ctx.split('.').collect();
    match p.as_slice() {
        ["arrange", _, "ruler", "click"] => "ruler.any.empty.click",
        _ => ctx,
    }
}

/// A context, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ctx {
    Gesture {
        area: String,
        tool: String,
        zone: String,
        gesture: String,
    },
    Hold {
        area: String,
        tool: String,
    },
}

impl Ctx {
    pub fn parse(text: &str) -> Result<Ctx, String> {
        let text = canonical(text);
        let p: Vec<&str> = text.split('.').collect();
        let bad = || {
            format!(
                "「{text}」は場所ではありません（<area>.<tool>.<zone>.<click|drag> か <area>.<tool>.hold）"
            )
        };
        let (area, tool) = match p.as_slice() {
            [a, t, ..] => (*a, *t),
            _ => return Err(bad()),
        };
        if !AREAS.iter().any(|x| x.name == area) {
            let names: Vec<&str> = AREAS.iter().map(|a| a.name).collect();
            return Err(format!(
                "「{area}」は場所ではありません（{}）",
                names.join(" / ")
            ));
        }
        if !tools(area).iter().any(|x| x.name == tool) {
            return Err(format!("「{tool}」は {area} のツールではありません"));
        }
        match p.as_slice() {
            [_, _, "hold"] if !has_holds(area) => {
                Err(format!("{area} にはツールの切り替えがありません"))
            }
            [_, _, "hold"] => Ok(Ctx::Hold {
                area: area.into(),
                tool: tool.into(),
            }),
            [_, _, zone, gesture] => {
                if !zones(area).iter().any(|z| z.name == *zone) {
                    return Err(format!("「{zone}」は {area} の部分ではありません"));
                }
                if !gestures(area).iter().any(|g| g.name == *gesture) {
                    let names: Vec<&str> = gestures(area).iter().map(|g| g.name).collect();
                    return Err(format!(
                        "「{gesture}」は {area} の操作ではありません（{}）",
                        names.join(" / ")
                    ));
                }
                Ok(Ctx::Gesture {
                    area: area.into(),
                    tool: tool.into(),
                    zone: (*zone).into(),
                    gesture: (*gesture).into(),
                })
            }
            _ => Err(bad()),
        }
    }

    pub fn is_hold(&self) -> bool {
        matches!(self, Ctx::Hold { .. })
    }

    /// The modifier in its written form, checked for this context.
    pub fn modifier(&self, text: &str) -> Result<String, String> {
        let (during, m) = match text.strip_prefix("during.") {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        if during && !matches!(self, Ctx::Gesture { gesture, .. } if gesture == "drag") {
            return Err("during.<修飾キー> はドラッグにだけ書けます".into());
        }
        let bits = Chord::parse_mods(m, Os::Win)?;
        if (self.is_hold() || during) && bits == 0 {
            return Err("修飾キーを書きます（Mod / Alt / Shift ...）".into());
        }
        let m = Chord::mods_text(bits);
        Ok(if during { format!("during.{m}") } else { m })
    }

    /// Checks what `action` may be here: a tool for a hold, an action for a gesture, `""` for
    /// nothing.
    pub fn action(&self, action: &str) -> Result<String, String> {
        let a = action.trim();
        if a.is_empty() || a == "none" {
            return Ok(String::new());
        }
        let a = a.split_whitespace().collect::<Vec<_>>().join(" ");
        match self {
            Ctx::Hold { .. } => {
                let t = a.strip_prefix("tool:").unwrap_or(&a);
                if TOOLS.iter().any(|x| x.name == t) {
                    Ok(format!("tool:{t}"))
                } else {
                    Err(format!(
                        "「{a}」はツールではありません（tool:select / tool:draw ...）"
                    ))
                }
            }
            Ctx::Gesture { area, .. } => {
                if !ACTIONS.iter().any(|(x, _)| *x == a) {
                    Err(format!("「{a}」はマウスに割り当てられません"))
                } else if !allowed_in(area, &a) {
                    Err(format!("「{a}」は {area} には割り当てられません"))
                } else {
                    Ok(a)
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mousemap {
    /// context → modifier → action, only where it differs from [`DEFAULTS`].
    pub changed: BTreeMap<String, BTreeMap<String, String>>,
}

/// A modifier as the screens write it (`Mod` is Ctrl on Windows).
fn mods_label(m: &str) -> String {
    m.replace("Mod", "Ctrl")
}

pub fn default_of(ctx: &str, modifier: &str) -> &'static str {
    DEFAULTS
        .iter()
        .find(|(c, m, _)| *c == ctx && *m == modifier)
        .map(|(_, _, a)| *a)
        .unwrap_or("")
}

/// Every context the defaults or the user write to.
pub fn contexts() -> Vec<String> {
    let mut out = Vec::new();
    for area in AREAS {
        for tool in tools(area.name) {
            for z in zones(area.name) {
                for g in gestures(area.name) {
                    out.push(format!("{}.{}.{}.{}", area.name, tool.name, z.name, g.name));
                }
            }
            if has_holds(area.name) {
                out.push(format!("{}.{}.hold", area.name, tool.name));
            }
        }
    }
    out
}

impl Mousemap {
    /// What `modifier` does in `ctx` (`""` for nothing).
    pub fn get(&self, ctx: &str, modifier: &str) -> String {
        let ctx = canonical(ctx);
        self.changed
            .get(ctx)
            .and_then(|m| m.get(modifier))
            .cloned()
            .unwrap_or_else(|| default_of(ctx, modifier).to_string())
    }

    /// Everything in effect in `ctx`, modifier → action, without the "nothing"s.
    pub fn table(&self, ctx: &str) -> BTreeMap<String, String> {
        let ctx = canonical(ctx);
        let mut out: BTreeMap<String, String> = DEFAULTS
            .iter()
            .filter(|(c, _, _)| *c == ctx)
            .map(|(_, m, a)| (m.to_string(), a.to_string()))
            .collect();
        if let Some(c) = self.changed.get(ctx) {
            for (m, a) in c {
                out.insert(m.clone(), a.clone());
            }
        }
        out.retain(|_, a| !a.is_empty());
        out
    }

    /// Sets what `modifier` does in `ctx` (checked; `""` for nothing).
    pub fn set(&mut self, ctx: &str, modifier: &str, action: &str) -> Result<(), String> {
        let c = Ctx::parse(ctx)?;
        let ctx = canonical(ctx);
        let m = c.modifier(modifier)?;
        let a = c.action(action)?;
        let entry = self.changed.entry(ctx.to_string()).or_default();
        if a == default_of(ctx, &m) {
            entry.remove(&m);
        } else {
            entry.insert(m, a);
        }
        if entry.is_empty() {
            self.changed.remove(ctx);
        }
        Ok(())
    }

    /// Back to what the app does: all, one context, or one modifier of it.
    pub fn reset(&mut self, ctx: Option<&str>, modifier: Option<&str>) -> Result<(), String> {
        let ctx = ctx.map(canonical);
        match (ctx, modifier) {
            (None, _) => self.changed.clear(),
            (Some(c), None) => {
                Ctx::parse(c)?;
                self.changed.remove(c);
            }
            (Some(c), Some(m)) => {
                let m = Ctx::parse(c)?.modifier(m)?;
                if let Some(e) = self.changed.get_mut(c) {
                    e.remove(&m);
                    if e.is_empty() {
                        self.changed.remove(c);
                    }
                }
            }
        }
        Ok(())
    }

    /// KEY-019: moves the tool a hold switches to from one modifier to another; the other
    /// modifier must be free (one tool per modifier in a tool's holds).
    pub fn move_hold(&mut self, ctx: &str, from: &str, to: &str) -> Result<(), String> {
        let c = Ctx::parse(ctx)?;
        if !c.is_hold() {
            return Err("修飾キーを移せるのはツールの切り替えだけです".into());
        }
        let (from, to) = (c.modifier(from)?, c.modifier(to)?);
        if from == to {
            return Ok(());
        }
        let moving = self.get(ctx, &from);
        if moving.is_empty() {
            return Err(format!("{} に割り当てはありません", mods_label(&from)));
        }
        let taken = self.get(ctx, &to);
        if !taken.is_empty() {
            return Err(format!(
                "{} は「{}」に使われています",
                mods_label(&to),
                action_label(&taken).unwrap_or("別のツール"),
            ));
        }
        self.set(ctx, &to, &moving)?;
        self.set(ctx, &from, "")
    }

    /// The tool `tool` switches to while `mods` (bits of [`crate::chord`]) are held in `area`;
    /// `None` keeps the tool.
    pub fn hold(&self, area: &str, tool: &str, mods: u8) -> Option<String> {
        if mods == 0 {
            return None;
        }
        let a = self.get(&format!("{area}.{tool}.hold"), &Chord::mods_text(mods));
        a.strip_prefix("tool:").map(String::from)
    }

    /// KEY-019: a held modifier that switches the tool hides the gestures on the same modifier.
    /// (context of the gesture, modifier, the tool it switches to).
    pub fn warnings(&self) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for area in AREAS.iter().filter(|a| has_holds(a.name)) {
            for tool in TOOLS {
                let holds = self.table(&format!("{}.{}.hold", area.name, tool.name));
                for z in zones(area.name) {
                    for g in gestures(area.name) {
                        let ctx = format!("{}.{}.{}.{}", area.name, tool.name, z.name, g.name);
                        for m in self.table(&ctx).keys() {
                            if let Some(to) = holds.get(m) {
                                out.push((ctx.clone(), m.clone(), to.clone()));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub fn changed_count(&self) -> usize {
        self.changed.values().map(BTreeMap::len).sum()
    }

    pub fn from_toml(t: &toml::Table, problems: &mut Vec<String>) -> Mousemap {
        let mut m = Mousemap::default();
        let mut seen: Vec<(String, String)> = Vec::new();
        for (path, v) in tomlw::flatten(t) {
            let (ctx, modifier) = split_path(&path);
            let Some(action) = v.as_str() else {
                problems.push(format!(
                    "mousemap.toml: {} は文字列にします",
                    path.join(".")
                ));
                continue;
            };
            let key = Ctx::parse(&ctx)
                .and_then(|c| c.modifier(&modifier))
                .map(|md| (canonical(&ctx).to_string(), md));
            if let Ok(k) = key {
                if seen.contains(&k) {
                    problems.push(format!(
                        "mousemap.toml: {} は重複しています（後の行を使います）",
                        path.join(".")
                    ));
                }
                seen.push(k);
            }
            if let Err(e) = m.set(&ctx, &modifier, action) {
                problems.push(format!("mousemap.toml: {} {e}", path.join(".")));
            }
        }
        m
    }

    pub fn to_toml(&self) -> String {
        let mut s = String::from("# mousemap.toml（既定からの差分。\"\" は何もしない）\n");
        let order = contexts();
        let mut ctxs: Vec<&String> = self.changed.keys().collect();
        ctxs.sort_by_key(|c| order.iter().position(|o| o == *c).unwrap_or(usize::MAX));
        for ctx in ctxs {
            s.push_str(&format!("\n[{ctx}]\n"));
            for (m, a) in &self.changed[ctx] {
                let key = match m.strip_prefix("during.") {
                    Some(rest) => format!("during.{}", tomlw::key(rest)),
                    None => tomlw::key(m),
                };
                s.push_str(&format!("{key} = {}\n", tomlw::string(a)));
            }
        }
        s
    }

    pub fn to_json(&self, ctx: Option<&str>) -> Value {
        let ctxs: Vec<String> = match ctx {
            Some(c) => vec![c.to_string()],
            None => contexts()
                .into_iter()
                .filter(|c| !self.table(c).is_empty() || self.changed.contains_key(c))
                .collect(),
        };
        let mut out = serde_json::Map::new();
        for c in ctxs {
            let rows: Vec<Value> = self
                .table(&c)
                .iter()
                .map(|(m, a)| {
                    json!({"modifier": m, "action": a, "label": action_label(a),
                        "changed": a != default_of(&c, m)})
                })
                .collect();
            out.insert(c, Value::Array(rows));
        }
        Value::Object(out)
    }
}

/// `[roll.select.note-body.drag] during.Shift = ...` flattens to
/// `roll.select.note-body.drag.during.Shift`: the context is the first three or four parts.
fn split_path(path: &[String]) -> (String, String) {
    let joined = path.join(".");
    let parts: Vec<&str> = joined.split('.').collect();
    let n = if parts.get(2) == Some(&"hold") { 3 } else { 4 };
    let n = n.min(parts.len());
    (parts[..n].join("."), parts[n..].join("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::{ALT, MOD, SHIFT};

    #[test]
    fn the_defaults_are_the_apps_holds() {
        let m = Mousemap::default();
        assert_eq!(m.hold("roll", "select", MOD).as_deref(), Some("draw"));
        assert_eq!(
            m.hold("roll", "select", MOD | SHIFT).as_deref(),
            Some("cut")
        );
        assert_eq!(m.hold("roll", "draw", MOD).as_deref(), Some("select"));
        assert_eq!(m.hold("arrange", "select", MOD).as_deref(), Some("range"));
        assert_eq!(m.hold("arrange", "cut", ALT).as_deref(), Some("select"));
        assert_eq!(m.hold("arrange", "select", ALT), None);
        assert!(m.warnings().is_empty(), "{:?}", m.warnings());
        for (c, md, a) in DEFAULTS {
            let ctx = Ctx::parse(c).unwrap();
            assert_eq!(ctx.modifier(md).unwrap(), *md);
            assert_eq!(ctx.action(a).unwrap(), *a);
        }
    }

    #[test]
    fn a_change_is_kept_as_the_difference() {
        let mut m = Mousemap::default();
        m.set("roll.select.hold", "Alt", "erase").unwrap();
        m.set("roll.select.hold", "Mod", "").unwrap();
        assert_eq!(m.hold("roll", "select", ALT).as_deref(), Some("erase"));
        assert_eq!(m.hold("roll", "select", MOD), None);
        // Setting the default back takes the line away.
        m.set("roll.select.hold", "Mod", "tool:draw").unwrap();
        assert_eq!(m.changed_count(), 1);
        assert!(m.set("roll.select.hold", "Alt", "note.move").is_err());
        assert!(
            m.set("roll.select.note-body.click", "during.Shift", "select.set")
                .is_err()
        );
        assert!(
            m.set("roll.select.clip-body.drag", "none", "clip.move")
                .is_err()
        );
    }

    #[test]
    fn the_lanes_automation_and_ruler_are_named_like_the_other_areas() {
        // 06 §3.1's own examples.
        for c in [
            "lane.select.velocity-bar.drag",
            "automation.select.point.drag",
            "ruler.any.empty.wheel",
            "arrange.select.ruler.click",
        ] {
            assert!(Ctx::parse(c).is_ok(), "{c}");
        }
        // The ruler's click was once the arrangement's: an older file still says so.
        let mut m = Mousemap::default();
        m.set("arrange.select.ruler.click", "none", "").unwrap();
        assert_eq!(m.get("ruler.any.empty.click", "none"), "");
        assert!(m.changed.contains_key("ruler.any.empty.click"));
        assert!(m.to_toml().contains("[ruler.any.empty.click]"));
        for c in ["lane.select.velocity-bar.drag", "ruler.any.empty.wheel"] {
            assert!(contexts().iter().any(|x| x == c), "{c}");
        }
        // Every context the app lists is read back, and the defaults only name listed ones.
        for c in contexts() {
            assert!(Ctx::parse(&c).is_ok(), "{c}");
        }
        let all = contexts();
        for (c, _, _) in DEFAULTS {
            assert!(all.iter().any(|x| x == c), "{c}");
        }
        // The ruler has no tool but `any`; the lane and automation have no holds.
        assert!(Ctx::parse("ruler.select.empty.click").is_err());
        assert!(Ctx::parse("ruler.any.empty.drag").is_ok());
        assert!(Ctx::parse("roll.select.note-body.wheel").is_err());
        assert!(Ctx::parse("lane.select.hold").is_err());
        assert!(Ctx::parse("automation.select.hold").is_err());
        assert!(Ctx::parse("lane.select.note-body.drag").is_err());
        assert!(Ctx::parse("lane.cut.empty.drag").is_err());
        let e = Ctx::parse("mixer.select.empty.drag").unwrap_err();
        assert!(e.contains("lane") && e.contains("ruler"), "{e}");
    }

    #[test]
    fn an_action_is_kept_to_the_areas_it_belongs_to() {
        let mut m = Mousemap::default();
        assert!(
            m.set("lane.select.cc-point.drag", "Alt", "cc.draw --line")
                .is_ok()
        );
        assert!(
            m.set("lane.select.velocity-bar.drag", "Mod", "note.velocity.set")
                .is_ok()
        );
        assert!(
            m.set("lane.select.cc-point.drag", "Mod", "note.move")
                .is_err()
        );
        assert!(
            m.set("roll.select.note-body.drag", "Mod", "cc.draw")
                .is_err()
        );
        assert!(
            m.set("automation.select.point.drag", "Alt", "auto.point.delete")
                .is_ok()
        );
        assert!(
            m.set("automation.select.point.drag", "Mod", "clip.move")
                .is_err()
        );
        assert!(
            m.set("ruler.any.empty.wheel", "Shift", "view.scroll")
                .is_ok()
        );
        assert!(
            m.set("ruler.any.empty.wheel", "Mod", "view.zoom.data")
                .is_ok()
        );
        assert!(m.set("ruler.any.empty.wheel", "none", "note.move").is_err());
        // A wheel is not a drag: no `during.`.
        assert!(
            m.set("ruler.any.empty.wheel", "during.Shift", "view.scroll")
                .is_err()
        );
        // Turning the ruler's wheel off is written as nothing.
        m.set("ruler.any.empty.wheel", "none", "").unwrap();
        assert_eq!(m.get("ruler.any.empty.wheel", "none"), "");
        assert!(!m.table("ruler.any.empty.wheel").contains_key("none"));
        assert_eq!(
            m.get("ruler.any.empty.wheel", "Shift"),
            "view.scroll",
            "{:?}",
            m.changed
        );
    }

    #[test]
    fn the_new_areas_read_back_from_the_file() {
        let mut m = Mousemap::default();
        m.set("lane.select.cc-point.drag", "during.Mod", "")
            .unwrap();
        m.set("automation.select.point.click", "Alt", "").unwrap();
        m.set("ruler.any.empty.wheel", "Shift", "view.scroll")
            .unwrap();
        let text = m.to_toml();
        assert!(text.contains("[ruler.any.empty.wheel]"), "{text}");
        let mut problems = Vec::new();
        let back = Mousemap::from_toml(&text.parse().unwrap(), &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, m);
        let j = m.to_json(Some("ruler.any.empty.wheel"));
        assert_eq!(j["ruler.any.empty.wheel"][0]["modifier"], "Shift");
        // A file naming a bad hit area is a problem, not a crash.
        let t: toml::Table = "[lane.select.note-body.drag]\nnone = \"note.move\"\n"
            .parse()
            .unwrap();
        let mut problems = Vec::new();
        let none = Mousemap::from_toml(&t, &mut problems);
        assert_eq!(none.changed_count(), 0);
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn the_ruler_drags_up_and_down_to_zoom_and_the_clips_take_what_follows() {
        let mut m = Mousemap::default();
        // A drag on the ruler zooms in time unless the map says otherwise.
        assert_eq!(m.get("ruler.any.empty.drag", "none"), "view.zoom.time");
        assert!(allowed_in("ruler", "view.zoom.data"));
        assert!(!allowed_in("ruler", "clip.move"));
        m.set("ruler.any.empty.drag", "Alt", "view.zoom.data")
            .unwrap();
        m.set("ruler.any.empty.drag", "Shift", "view.scroll")
            .unwrap();
        assert!(m.set("ruler.any.empty.drag", "Mod", "note.move").is_err());
        m.set("ruler.any.empty.drag", "none", "").unwrap();
        assert_eq!(m.get("ruler.any.empty.drag", "none"), "");
        let text = m.to_toml();
        let mut problems = Vec::new();
        let back = Mousemap::from_toml(&text.parse().unwrap(), &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, m);
        // Alt on a clip or on the empty part of the arrangement takes the clips after the pointer,
        // as it takes the roll's notes; the lanes have none.
        let m = Mousemap::default();
        assert_eq!(
            m.get("arrange.select.clip-body.click", "Alt"),
            "select.after"
        );
        assert_eq!(m.get("arrange.select.empty.click", "Alt"), "select.after");
        assert!(allowed_in("arrange", "select.after"));
        assert!(allowed_in("roll", "select.after"));
        assert!(!allowed_in("lane", "select.after"));
        assert!(!allowed_in("automation", "select.after"));
        assert!(!allowed_in("ruler", "select.after"));
    }

    #[test]
    fn a_hold_on_a_gestures_modifier_is_a_warning() {
        let mut m = Mousemap::default();
        m.set("roll.select.hold", "Alt", "tool:erase").unwrap();
        let w = m.warnings();
        assert!(w.iter().any(|(c, md, to)| c == "roll.select.note-body.drag"
            && md == "Alt"
            && to == "tool:erase"));
    }

    #[test]
    fn the_file_reads_back() {
        let mut m = Mousemap::default();
        m.set("roll.select.note-edge.drag", "during.Mod+Alt", "")
            .unwrap();
        m.set("roll.select.hold", "Mod+Shift", "tool:erase")
            .unwrap();
        m.set("arrange.select.clip-body.drag", "Mod", "clip.slip")
            .unwrap();
        let text = m.to_toml();
        assert!(text.contains("during.\"Mod+Alt\" = \"\""), "{text}");
        let mut problems = Vec::new();
        let back = Mousemap::from_toml(&text.parse().unwrap(), &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, m);
        // The quoted form reads too.
        let t: toml::Table = "[\"roll.select.hold\"]\nAlt = \"tool:mute\"\n[roll.select.note-body.drag]\n\"during.Alt\" = \"snap.invert\"\n"
            .parse()
            .unwrap();
        let q = Mousemap::from_toml(&t, &mut problems);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(q.hold("roll", "select", ALT).as_deref(), Some("mute"));
        assert_eq!(
            q.get("roll.select.note-body.drag", "during.Alt"),
            "snap.invert"
        );
    }
}
