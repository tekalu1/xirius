//! Icon names come from the command table, with the palette's current key.
use crate::draw::{Align, Pen};
use crate::studio::{DrawerTab, Geometry, Hit, Rect, Studio};
use crate::topbar::BarButton;
use xirius_design::{FontSize, Radius, Space, Weight};

pub fn command(hit: Hit) -> Option<String> {
    Some(
        match hit {
            Hit::Mute(_) => "track.mute",
            Hit::Solo(_) => "track.solo",
            Hit::Arm(_) => "track.arm",
            Hit::Monitor(_) => "track.monitor",
            Hit::Keys(_) => "view.instrument",
            Hit::TrackColor(_) => "track.color.set",
            Hit::Widen(_) => "view.track-details",
            Hit::Freeze(_) => "track.freeze",
            Hit::Output(_) => "track.output.set",
            Hit::FolderBus(_) => "folder.bus.set",
            Hit::Chevron(_) => "view.folder",
            Hit::AddTrack => "view.add-track",
            Hit::ZoomIn | Hit::ZoomOut => "view.zoom.time",
            Hit::WaveZoom => "view.zoom.wave",
            Hit::Follow => "view.autoscroll",
            Hit::Fold => "view.tracklist.collapse",
            Hit::ToggleArrange => "view.sections",
            Hit::ToggleTempo => "view.tempo",
            Hit::Rec(part) => part.command(),
            Hit::RailSettings => "view.settings",
            Hit::Tab(DrawerTab::Roll) => "clip.open",
            Hit::Tab(DrawerTab::Mixer) => "view.mixer",
            Hit::RollView(_) | Hit::RollMap => "roll.view",
            Hit::RollLength(_) => "prefs.set",
            Hit::Bar(b) => match b {
                BarButton::Tool(t) => return Some(format!("tool.{}", t.name())),
                BarButton::AltTool => "tool.alt",
                BarButton::Shape => "tool.draw.shape",
                BarButton::DrawLength => "prefs.set",
                BarButton::Logo => return None,
                BarButton::SongName => "project.open",
                BarButton::Saved => "version.list",
                BarButton::Grid => "view.grid",
                BarButton::Snap => "view.snap",
                BarButton::History => "history.list",
                BarButton::Palette => "view.palette",
                BarButton::Ai => "ai.permission.set",
                BarButton::Export => "export.mixdown",
                BarButton::ToStart => "transport.seek",
                BarButton::Play => "transport.play",
                BarButton::Stop => "transport.stop",
                BarButton::Record => "record.start",
                BarButton::Loop => "transport.loop",
                BarButton::PreRoll => "record.preroll",
                BarButton::RecPanel => "view.record-panel",
                BarButton::Cpu => "engine.stats",
                BarButton::Metronome => "transport.metronome",
                BarButton::Tempo => "tempo.set",
                BarButton::Meter => "timesig.set",
                BarButton::Ripple => "edit.ripple",
            },
            Hit::Mixer(m) => {
                return match m {
                    crate::mixer::MixerHit::Mute(t) => command(Hit::Mute(t)),
                    crate::mixer::MixerHit::Solo(t) => command(Hit::Solo(t)),
                    crate::mixer::MixerHit::Arm(t) => command(Hit::Arm(t)),
                    crate::mixer::MixerHit::Fader(_) => Some("track.volume.set".into()),
                    crate::mixer::MixerHit::Pan(_) => Some("track.pan.set".into()),
                    crate::mixer::MixerHit::Output(_) => Some("track.output.set".into()),
                    crate::mixer::MixerHit::Input(_) => Some("track.input.set".into()),
                    crate::mixer::MixerHit::AddInsert(_) => Some("plugin.add".into()),
                    crate::mixer::MixerHit::AddInstrument(_) => Some("view.instrument".into()),
                    crate::mixer::MixerHit::AddSend(_) => Some("send.add".into()),
                    crate::mixer::MixerHit::SendLevel(..) => Some("send.set".into()),
                    crate::mixer::MixerHit::SendPre(..) => Some("send.set".into()),
                    crate::mixer::MixerHit::Bypass(..) => Some("plugin.bypass".into()),
                    crate::mixer::MixerHit::Open(..) => Some("plugin.open".into()),
                    crate::mixer::MixerHit::Strip(_) => None,
                };
            }
            Hit::Browser(b) => match b {
                crate::browser::BrowserHit::Fav(_) => "browser.favorite",
                crate::browser::BrowserHit::LocAdd => "location.add",
                crate::browser::BrowserHit::LocRemove(_) => "location.remove",
                crate::browser::BrowserHit::StopScan => "plugin.rescan.cancel",
                crate::browser::BrowserHit::ViewButton => "view.browser.sort",
                crate::browser::BrowserHit::Audition => "browser.audition",
                _ => return None,
            },
            _ => return None,
        }
        .into(),
    )
}

pub fn popup(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let Hit::Item(i) = s.hover else {
        return;
    };
    let Some(p) = &s.popup else {
        return;
    };
    let layout = p.layout(g.size);
    let Some(cell) = layout
        .cells
        .get(i)
        .filter(|c| c.tip.is_empty() && c.text.is_empty() && c.icon.is_some())
    else {
        return;
    };
    use crate::popup::Action;
    let name = match cell.action {
        Some(Action::Undo) => "history.undo",
        Some(Action::Redo) => "history.redo",
        Some(Action::AutoQuantize) => "note.quantize",
        Some(Action::Save) => "project.save",
        _ => return,
    };
    if let Some(label) = s.song.registry().label(name) {
        let key = s.menu_key(name);
        let tip = if key.is_empty() {
            label.into()
        } else {
            format!("{label}  {key}")
        };
        balloon(
            pen,
            &tip,
            cell.rect,
            Rect::new(0.0, 0.0, g.size.0, g.size.1),
        );
    }
}

pub fn text(s: &Studio, hit: Hit) -> Option<String> {
    let label = match hit {
        Hit::RollView(false) => Some("鍵盤の表示"),
        Hit::RollView(true) => Some("ドラムの表示"),
        Hit::RollMap => Some("ドラムのマップ"),
        Hit::RollLength(_) => Some("直前のノートの長さで入力"),
        Hit::Bar(BarButton::DrawLength) => Some("グリッドの長さで入力"),
        _ => None,
    };
    if let Some(label) = label {
        return Some(label.into());
    }

    if let Hit::Rail(tab) = hit {
        return Some(rail_text(s, tab));
    }
    // The audition's stop button, while the file is read and nothing is heard (BRW-023).
    if hit == Hit::Browser(crate::browser::BrowserHit::Audition)
        && s.browser
            .playing
            .as_deref()
            .is_some_and(crate::audition::waiting)
    {
        return Some(crate::audition::READING.into());
    }
    // The pill after a track's name says the values it stands for (UI-312).
    if let Hit::TrackMark(t) = hit {
        return crate::draw::track_mark_tip(s.project().tracks.get(&t)?);
    }
    // The row of a strip without an instrument shows only an icon (UI-305).
    if let Hit::Mixer(crate::mixer::MixerHit::AddInstrument(_)) = hit {
        return Some(crate::instrument::PICK_LABEL.into());
    }
    // The vertical zoom says how far it is (UI-310), as the mock's ×1・×2・×4.
    if hit == Hit::WaveZoom {
        let label = xirius_core::commands::labels::label("view.zoom.wave")?;
        return Some(format!("{label} ×{}", s.wave_zoom));
    }
    named(s, &command(hit)?)
}

/// A rail button says its tab, with the key that opens that tab when there is one (no key for
/// the pool).
fn rail_text(s: &Studio, tab: crate::browser::RailTab) -> String {
    let key = ["browser.tab", "view.browser"]
        .into_iter()
        .map(|command| s.menu_key(&format!("{command} --tab {}", tab.name())))
        .find(|key| !key.is_empty())
        .unwrap_or_default();
    if key.is_empty() {
        tab.label().into()
    } else {
        format!("{}  {key}", tab.label())
    }
}

fn named(s: &Studio, command: &str) -> Option<String> {
    let label = s
        .song
        .registry()
        .label(command)
        .or_else(|| xirius_core::commands::labels::label(command))?;
    let key = s.menu_key(command);
    Some(if key.is_empty() {
        label.into()
    } else {
        format!("{label}  {key}")
    })
}

pub fn balloon(pen: &mut Pen, text: &str, at: Rect, within: Rect) {
    if text.is_empty() {
        return;
    }
    let pad = Space::X2.px();
    let w = (pen.measure(text, FontSize::Caption, Weight::Medium) + pad * 2.0).min(within.w);
    let x = (at.x + (at.w - w) / 2.0).clamp(within.x, (within.right() - w).max(within.x));
    let y = (at.bottom() + Space::X1.px())
        .min(within.bottom() - 26.0)
        .max(within.y);
    let r = Rect::new(x, y, w, 26.0);
    pen.floating(r, pen.c.surface_3, Radius::S);
    pen.mask(r, |pen| {
        pen.text(
            text,
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink_strong,
            Rect::new(x + pad, y, w - 2.0 * pad, 26.0),
            Align::Left,
        );
    });
}

pub fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) {
    if s.popup.is_some() || s.palette.is_some() || s.settings.is_some() {
        return;
    }
    // A control its lane holds says why it does not move (UI-063).
    if let Some(rect) = s.lock_hovered(g) {
        balloon(
            pen,
            crate::auto_lock::REASON,
            rect,
            Rect::new(0.0, 0.0, g.size.0, g.size.1),
        );
        return;
    }
    if let Some((name, rect)) = roll_control(s, g) {
        if let Some(tip) = named(s, name) {
            balloon(pen, &tip, rect, Rect::new(0.0, 0.0, g.size.0, g.size.1));
        }
        return;
    }
    let hit = hovered(s, g);
    if let Some(text) = folded_text(s, hit)
        && let Some(rect) = anchor(s, g, hit)
    {
        balloon(pen, &text, rect, Rect::new(0.0, 0.0, g.size.0, g.size.1));
        return;
    }
    if let Some(text) = s.pool_tip(g, hit)
        && let Some(rect) = anchor(s, g, hit)
    {
        balloon(pen, &text, rect, Rect::new(0.0, 0.0, g.size.0, g.size.1));
        return;
    }
    if hit == Hit::RailSettings {
        settings_tip(pen, s, g);
        return;
    }
    if let Some(text) = text(s, hit)
        && let Some(rect) = anchor(s, g, hit)
    {
        balloon(pen, &text, rect, Rect::new(0.0, 0.0, g.size.0, g.size.1));
    }
}

/// UI-066: what the track list's fold says under the pointer. A row of the folded strip gives
/// the name the strip leaves out; the arrow says what it does now (the command's one label
/// says "fold"), with the key of the arrangement whatever has the focus.
pub(crate) fn folded_text(s: &Studio, hit: Hit) -> Option<String> {
    let project = s.project();
    match hit {
        Hit::Folded(crate::tree::RowKind::Track(t)) => {
            project.tracks.get(&t).map(|t| t.name.clone())
        }
        Hit::Folded(crate::tree::RowKind::Folder(f)) => {
            project.folders.get(&f).map(|f| f.name.clone())
        }
        Hit::Fold => {
            let key = s.menu_key_in("global", "view.tracklist.collapse");
            let label = if s.side_size.folded {
                "トラック一覧を広げる"
            } else {
                "トラック一覧を畳む"
            };
            Some(if key.is_empty() {
                label.into()
            } else {
                format!("{label}  {key}")
            })
        }
        _ => None,
    }
}

/// The gear at the foot of the rail: one line, at its right, with the latency when it is known.
fn settings_tip(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let button = crate::browser::rail_io_button(g.rail);
    let key = s.menu_key("view.settings");
    let name = s.io_tooltip();
    let text = if key.is_empty() {
        name
    } else {
        format!("{name}  {key}")
    };
    let pad = Space::X2.px();
    let w = pen.measure(&text, FontSize::Caption, Weight::Medium) + pad * 2.0;
    let x = button.right() + Space::X1.px();
    let y = (button.y + (button.h - 26.0) / 2.0).clamp(0.0, (g.size.1 - 26.0).max(0.0));
    let r = Rect::new(x, y, w, 26.0);
    pen.floating(r, pen.c.surface_3, Radius::S);
    pen.mask(r, |pen| {
        pen.text(
            &text,
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink_strong,
            Rect::new(x + pad, y, w - 2.0 * pad, 26.0),
            Align::Left,
        );
    });
}

fn hovered(s: &Studio, g: &Geometry) -> Hit {
    let (x, y) = s.input_map.pointer;
    if matches!(s.hover, Hit::None | Hit::Row(_))
        && g.column.contains(x, y)
        && let Some(row) = s.tree().row_at(y - g.column.y + s.scroll.1)
        && let crate::tree::RowKind::Track(t) = row.kind
    {
        for hit in [Hit::Monitor(t), Hit::Keys(t)] {
            if anchor(s, g, hit).is_some_and(|r| r.contains(x, y)) {
                return hit;
            }
        }
    }
    s.hover
}

pub(crate) fn pointer_tip(s: &Studio, g: &Geometry) -> Option<(&'static str, Rect)> {
    if s.popup.is_some() {
        return None;
    }
    if let Some(r) = s.lock_hovered(g) {
        return Some((crate::auto_lock::REASON, r));
    }
    if let Some(tip) = roll_control(s, g) {
        return Some(tip);
    }
    let hit = hovered(s, g);
    let name = match hit {
        Hit::Monitor(_) => "track.monitor",
        Hit::Keys(_) => "view.instrument",
        _ => return None,
    };
    anchor(s, g, hit).map(|r| (name, r))
}

fn anchor(s: &Studio, g: &Geometry, hit: Hit) -> Option<Rect> {
    if let Hit::Monitor(t) | Hit::Keys(t) = hit {
        let row = s.tree().row_of(crate::tree::RowKind::Track(t))?;
        let parts = crate::studio::RowParts::of(row);
        let r = if matches!(hit, Hit::Monitor(_)) {
            parts.ear?
        } else {
            parts.keys?
        };
        let r = Rect::new(g.column.x + r.x, s.row_y(g, row) + r.y, r.w, r.h);
        return g
            .column
            .contains(r.x + r.w / 2.0, r.y + r.h / 2.0)
            .then_some(r);
    }
    s.center_of(g, hit).map(|(x, y)| Rect::new(x, y, 0.0, 12.0))
}

fn roll_control(s: &Studio, g: &Geometry) -> Option<(&'static str, Rect)> {
    if !s.drawer_open || s.tab != DrawerTab::Roll {
        return None;
    }
    let f = s.roll.frame(g.drawer_body);
    let (x, y) = s.input_map.pointer;
    [
        (f.plus, "view.controller.add"),
        (f.fold, "view.controller.fold"),
        (f.follow, "view.autoscroll"),
    ]
    .into_iter()
    .find(|(r, _)| r.contains(x, y))
    .map(|(r, name)| (name, r))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The mark after a track's name is under the pointer and says its two values (UI-312).
    #[test]
    fn the_tracks_mark_says_the_values_it_stands_for() {
        let mut s = Studio::sample();
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        s.song
            .run(
                "track.transpose",
                serde_json::json!({"ids": [t.to_string()], "semis": -12}),
            )
            .unwrap();
        s.song
            .run(
                "track.velocity",
                serde_json::json!({"ids": [t.to_string()], "percent": 10}),
            )
            .unwrap();
        let g = s.geometry(1440.0, 860.0);
        let row = *s
            .tree()
            .row_of(crate::tree::RowKind::Track(t))
            .expect("the row");
        let p = s.project();
        let track = &p.tracks[&t];
        let mark = s.row_mark(&row, track);
        assert!(mark.text.is_some(), "a mark");
        let r = crate::draw::track_mark_rect(&row, &mark);
        let at = (
            g.column.x + r.x + r.w / 2.0,
            s.row_y(&g, &row) + r.y + r.h / 2.0,
        );
        assert_eq!(s.hit(&g, at.0, at.1), Hit::TrackMark(t));
        assert_eq!(
            text(&s, Hit::TrackMark(t)).as_deref(),
            Some("トランスポーズ −12・ベロシティ +10%")
        );
    }

    #[test]
    fn every_bar_icon_and_track_switch_uses_a_palette_label() {
        let s = Studio::sample();
        for hit in [
            Hit::Bar(BarButton::Play),
            Hit::Bar(BarButton::Export),
            Hit::Solo(xirius_core::id::TrackId::MASTER),
            Hit::RailSettings,
        ] {
            let name = command(hit).unwrap();
            assert!(
                text(&s, hit)
                    .unwrap()
                    .starts_with(s.song.registry().label(&name).unwrap())
            );
        }
        for name in [
            "plugin.open",
            "plugin.rescan.cancel",
            "view.controller.add",
            "view.controller.fold",
        ] {
            assert!(named(&s, name).is_some(), "{name}");
        }
        let mut roll = Studio::sample();
        let g = roll.geometry(1280.0, 800.0);
        let f = roll.roll.frame(g.drawer_body);
        for rect in [f.plus, f.fold, f.follow] {
            roll.input_map.pointer = (rect.x + 2.0, rect.y + 2.0);
            assert!(roll_control(&roll, &g).is_some());
        }
        let t = roll
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Drums")
            .unwrap()
            .id;
        let r = anchor(&roll, &g, Hit::Monitor(t)).unwrap();
        roll.input_map.pointer = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        roll.hover = roll.hit(&g, roll.input_map.pointer.0, roll.input_map.pointer.1);
        assert_eq!(roll.hover, Hit::None);
        assert_eq!(hovered(&roll, &g), Hit::Monitor(t));
        assert!(text(&roll, hovered(&roll, &g)).is_some());
        let monitor_tip = pointer_tip(&roll, &g).unwrap();
        roll.input_map.pointer = (g.column.x + 2.0, g.column.y + 2.0);
        assert_ne!(pointer_tip(&roll, &g), Some(monitor_tip));
        for t in crate::tools::Tool::ALL {
            assert!(text(&s, Hit::Bar(BarButton::Tool(t))).is_some());
        }
    }

    #[test]
    fn each_rail_button_says_its_own_tab_and_the_key_that_opens_it() {
        use crate::browser::RailTab;
        let s = Studio::sample();
        let tips: Vec<String> = RailTab::ALL
            .iter()
            .map(|&tab| text(&s, Hit::Rail(tab)).unwrap())
            .collect();
        assert_eq!(
            tips,
            [
                "インストゥルメント  F6",
                "エフェクト  F7",
                "ファイル  F9",
                "プール",
                "インスペクター  F4"
            ]
        );
        assert!(tips.iter().all(|t| !t.contains("ブラウザーを表示")));
    }
}
