//! The track list as an accessible tree (UI-026): every row a TreeItem named after the track
//! or folder it shows (folders carrying their folded state and depth), so a screen reader
//! reads the list. Only while the settings are closed — they cover the column.
//! Over the picture, the controls a screen reader could not reach (UI-298): the search box of
//! the effects and instruments lists, and each clip's count of takes.
use crate::studio::{Geometry, Rect, Studio};
use crate::tree::RowKind;
use gpui::accesskit;
use xirius_core::id::TrackId;
use xirius_design::Layout;

/// The rows of the track column as accessible nodes (window coordinates).
pub fn track_rows(s: &Studio, g: &Geometry) -> Vec<(usize, accesskit::Node)> {
    if s.settings.is_some() {
        return Vec::new();
    }
    let p = s.project();
    s.tree()
        .rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| {
            let y0 = (g.column.y + row.y - s.scroll.1).max(g.column.y);
            let y1 = (g.column.y + row.y + row.h - s.scroll.1).min(g.column.bottom());
            if y1 - y0 <= 1.0 {
                return None;
            }
            let name = match row.kind {
                RowKind::Track(t) => p.tracks.get(&t)?.name.clone(),
                RowKind::Folder(f) => p.folders.get(&f)?.name.clone(),
            };
            let mut node = accesskit::Node::new(accesskit::Role::TreeItem);
            node.set_label(name);
            node.set_level(row.depth + 1);
            node.set_selected(s.selected_row == Some(row.kind));
            if let RowKind::Folder(f) = row.kind {
                node.set_expanded(!s.is_collapsed(f));
            }
            node.set_bounds(accesskit::Rect::new(
                g.column.x as f64,
                (y0 + Layout::TOP_BAR_H) as f64,
                g.column.right() as f64,
                (y1 + Layout::TOP_BAR_H) as f64,
            ));
            Some((i, node))
        })
        .collect()
}

/// The open settings as accessible nodes: the sections, the controls of the page (switches,
/// choices, dropdowns, fields, buttons) each named after the label painted by it, so a screen
/// reader reads the page and not an empty window. The audio inputs come as RadioButtons.
pub fn settings_nodes(s: &Studio, g: &Geometry) -> Vec<(usize, accesskit::Node)> {
    use crate::settings::Do;
    use crate::settings::scene::Pick;
    let Some(ui) = s.settings.as_ref() else {
        return Vec::new();
    };
    let scene = s.settings_scene(g);
    let texts = scene.text_boxes();
    let mut out: Vec<(usize, accesskit::Node)> = crate::settings::audio::radios(s, g);
    for (i, item) in scene.items.iter().enumerate() {
        let r = item.r;
        // Seen: not scrolled out of its list.
        if !item.clip.contains(r.x + r.w / 2.0, r.y + r.h / 2.0) {
            continue;
        }
        let inside = |t: &Rect| {
            let (cx, cy) = (t.x + t.w / 2.0, t.y + t.h / 2.0);
            r.contains(cx, cy)
        };
        let own: Vec<&str> = texts
            .iter()
            .filter(|(_, t)| inside(t))
            .map(|(t, _)| *t)
            .collect();
        // The label of the row it is in: the text that starts nearest to its left on the same
        // line, in the page (a label's box runs to the row's end, so its start is what counts).
        let row = texts
            .iter()
            .filter(|(_, t)| {
                let cy = t.y + t.h / 2.0;
                cy >= r.y && cy <= r.bottom() && t.x < r.x && t.x >= scene.body.x - 1.0
            })
            .max_by(|a, b| a.1.x.total_cmp(&b.1.x))
            .map(|(t, _)| t.to_string());
        let own_text = own.join(" ");
        let (role, label, value) = match (&item.act, item.pick) {
            // The radios come above; a press that does nothing (the modal's own box) is no control.
            (Do::Input(_) | Do::Nothing, _) => continue,
            (Do::Nav(section), _) => (accesskit::Role::Tab, section.label().to_string(), None),
            (Do::Close, _) => (accesskit::Role::Button, "閉じる".to_string(), None),
            (Do::Menu(_), _) => (
                accesskit::Role::ComboBox,
                row.unwrap_or_else(|| own_text.clone()),
                Some(own_text.clone()),
            ),
            (Do::Field(_), _) => (
                accesskit::Role::TextInput,
                row.unwrap_or_else(|| own_text.clone()),
                Some(own_text.clone()),
            ),
            (_, Some(Pick::Switch(_))) => (
                accesskit::Role::Switch,
                row.or(item.tip.clone()).unwrap_or(own_text.clone()),
                None,
            ),
            (_, Some(Pick::Choice(_))) => (
                accesskit::Role::RadioButton,
                if own_text.is_empty() {
                    item.tip.clone().unwrap_or_default()
                } else {
                    own_text.clone()
                },
                None,
            ),
            _ => (
                accesskit::Role::Button,
                if own_text.is_empty() {
                    item.tip.clone().or(row).unwrap_or_default()
                } else {
                    own_text.clone()
                },
                None,
            ),
        };
        let mut node = accesskit::Node::new(role);
        if !label.is_empty() {
            node.set_label(label);
        }
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            node.set_value(v);
        }
        match (&item.act, item.pick) {
            (Do::Nav(section), _) => node.set_selected(*section == ui.section),
            (_, Some(Pick::Switch(on) | Pick::Choice(on))) => node.set_toggled(if on {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            }),
            _ => {}
        }
        node.set_bounds(accesskit::Rect::new(
            r.x as f64,
            (r.y + Layout::TOP_BAR_H) as f64,
            r.right() as f64,
            (r.bottom() + Layout::TOP_BAR_H) as f64,
        ));
        // Ids stay apart from the audio radios' (their own indexes).
        out.push((1000 + i, node));
    }
    out
}

/// A control painted over the window, as an accessible node of its own (window coordinates).
pub struct Mark {
    /// Stays the same from frame to frame, apart from every other mark's.
    pub key: String,
    pub node: accesskit::Node,
    /// The track whose take rows a press on it opens or folds.
    pub fold: Option<TrackId>,
    /// The track whose instrument a press on it picks (the strip's icon-only row, UI-305).
    pub pick: Option<TrackId>,
}

fn bounds(r: Rect) -> accesskit::Rect {
    accesskit::Rect::new(
        r.x as f64,
        (r.y + Layout::TOP_BAR_H) as f64,
        r.right() as f64,
        (r.bottom() + Layout::TOP_BAR_H) as f64,
    )
}

/// The marks of what shows now (UI-298). An open list of effects or instruments has its search
/// box: an Edit named 「検索」, whose value is what was typed. Otherwise each clip's count of
/// takes on screen is a Button named 「テイク N」 that opens the take rows, expanded while they
/// are open. A pop-over takes the presses, so the counts under it are not offered.
pub fn marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    if s.settings.is_some() {
        return Vec::new();
    }
    if let Some(p) = &s.popup {
        // The value fields at the head of a clip's menu: a screen reader reads the name, the
        // value and the unit, which the field itself does not draw (UI-311).
        if p.kind == crate::popup::PopupKind::Context
            && s.ctx.as_ref().is_none_or(|c| c.ask.is_none())
        {
            return p
                .layout(g.size)
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.spoken.is_empty())
                .map(|(i, c)| {
                    let mut node = accesskit::Node::new(accesskit::Role::SpinButton);
                    node.set_label(c.spoken.clone());
                    node.set_bounds(bounds(c.rect));
                    Mark {
                        key: format!("field-{i}"),
                        node,
                        fold: None,
                        pick: None,
                    }
                })
                .collect();
        }
        if !crate::fx_picker::searched(p.kind) {
            return Vec::new();
        }
        let layout = p.layout(g.size);
        return layout
            .cells
            .iter()
            .filter(|c| c.look == crate::popup::Look::Input && c.icon == Some("search"))
            .map(|c| {
                let mut node = accesskit::Node::new(accesskit::Role::TextInput);
                node.set_label("検索");
                node.set_value(p.fx_query.clone());
                node.set_bounds(bounds(c.rect));
                Mark {
                    key: "search".into(),
                    node,
                    fold: None,
                    pick: None,
                }
            })
            .collect();
    }
    let p = s.project();
    let mut out = Vec::new();
    for row in &s.tree().rows {
        let RowKind::Track(t) = row.kind else {
            continue;
        };
        let Some(track) = p.tracks.get(&t) else {
            continue;
        };
        let stacks = xirius_core::takes::stacks(track);
        for clip in &track.clips {
            let Some(n) = crate::draw::takes_under(&stacks, clip) else {
                continue;
            };
            let Some(badge) = crate::draw::takes_badge(s.clip_rect(g, row, clip)) else {
                continue;
            };
            if !g
                .lanes
                .contains(badge.x + badge.w / 2.0, badge.y + badge.h / 2.0)
            {
                continue;
            }
            let mut node = accesskit::Node::new(accesskit::Role::Button);
            node.set_label(format!("テイク {n}"));
            node.set_expanded(s.takes_open.contains(&t));
            node.set_bounds(bounds(badge));
            out.push(Mark {
                key: format!("takes-{}", clip.id),
                node,
                fold: Some(t),
                pick: None,
            });
        }
    }
    out.extend(inspector_marks(s, g));
    out.extend(instrument_marks(s, g));
    out
}

/// The inspector's value fields, each a SpinButton named with its unit (「トランスポーズ +2
/// 半音」, UI-312): the field itself draws no unit (UI-311).
fn inspector_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    if !s.inspector_open() {
        return Vec::new();
    }
    s.inspector_layout(g)
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let text = crate::inspector::field_text(s, i, row);
            let mut node = accesskit::Node::new(accesskit::Role::SpinButton);
            node.set_label(row.kind.spoken(&text));
            node.set_bounds(bounds(crate::inspector::field_rect(row.rect)));
            Mark {
                key: format!("inspector-{i}"),
                node,
                fold: None,
                pick: None,
            }
        })
        .collect()
}

/// The mixer's row of a strip without an instrument shows only an icon, so it is a Button named
/// 「インストゥルメントを選ぶ」 (UI-305). Only a row on screen has one.
fn instrument_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::mixer::MixerHit;
    let p = s.project();
    s.mixer_strips(g)
        .into_iter()
        .filter(|strip| {
            p.tracks
                .get(&strip.track)
                .is_some_and(|t| crate::instrument::gap(t) == 1)
        })
        .filter_map(|strip| {
            let hit = MixerHit::AddInstrument(strip.track);
            s.mixer_center(g, hit)?;
            let mut node = accesskit::Node::new(accesskit::Role::Button);
            node.set_label(crate::instrument::PICK_LABEL);
            node.set_bounds(bounds(s.mixer_rect(g, hit)?));
            Some(Mark {
                key: format!("instrument-{}", strip.track),
                node,
                fold: None,
                pick: Some(strip.track),
            })
        })
        .collect()
}

/// A screen reader's press on the icon-only row: the same press as the pointer's.
pub fn pick(s: &mut Studio, g: &Geometry, track: TrackId) -> bool {
    s.press(
        g,
        crate::studio::Hit::Mixer(crate::mixer::MixerHit::AddInstrument(track)),
    )
}

/// 「Xirius について」 while it is open: its buttons and its words, in the window's
/// coordinates (UI-303). A button that has only a mark (the repository's) reads the first line
/// of its balloon, and the second is its description.
pub fn about_nodes(s: &Studio, g: &Geometry) -> Vec<(usize, accesskit::Node)> {
    let Some(popup) = s
        .popup
        .as_ref()
        .filter(|p| p.kind == crate::popup::PopupKind::About)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, cell) in popup.layout(g.size).cells.iter().enumerate() {
        let mut lines = cell.tip.lines();
        let (role, label) = if cell.live() {
            let label = if cell.text.is_empty() {
                lines.next().unwrap_or_default().to_string()
            } else {
                cell.text.clone()
            };
            (accesskit::Role::Button, label)
        } else if cell.look == crate::popup::Look::About(crate::about::AboutLook::Title) {
            (accesskit::Role::Heading, cell.text.clone())
        } else {
            (accesskit::Role::Label, cell.text.clone())
        };
        if label.is_empty() {
            continue;
        }
        let mut node = accesskit::Node::new(role);
        node.set_label(label);
        if let Some(more) = lines.next() {
            node.set_description(more.to_string());
        }
        let r = cell.rect;
        node.set_bounds(accesskit::Rect::new(
            r.x as f64,
            (r.y + Layout::TOP_BAR_H) as f64,
            r.right() as f64,
            (r.bottom() + Layout::TOP_BAR_H) as f64,
        ));
        out.push((i, node));
    }
    out
}

/// A screen reader's press on a count of takes: opens the take rows of `track`, or folds them.
pub fn fold(s: &mut Studio, track: TrackId) -> bool {
    s.rec_down(track, crate::rec_area::RecPart::Fold)
}

/// The bottom bar's buttons as accessible nodes, in the window's coordinates (UI-300): each
/// named after its balloon's first words, the switches with their state, and the position, the
/// tempo, the meter and the CPU with what they read (the CPU as "N%", UI-301).
pub fn bottom_bar_nodes(s: &Studio, g: &Geometry) -> Vec<(usize, accesskit::Node)> {
    use crate::studio::Hit;
    use crate::topbar::{BarButton, BarLayout};
    let v = s.bar_view();
    let layout = s.bar_layout(g.size.0);
    let mut out = Vec::new();
    let bounds = |r: Rect| {
        let y = g.size.1 + Layout::TOP_BAR_H;
        accesskit::Rect::new(
            r.x as f64,
            (r.y + y) as f64,
            r.right() as f64,
            (r.bottom() + y) as f64,
        )
    };
    let mut time = accesskit::Node::new(accesskit::Role::Label);
    time.set_label("位置");
    time.set_value(format!("{} {}", v.time, v.seconds).trim().to_string());
    time.set_bounds(bounds(layout.time));
    out.push((0, time));
    for (i, b) in BarLayout::BUTTONS.into_iter().enumerate() {
        if !b.in_bottom() {
            continue;
        }
        let r = layout.rect(b);
        if r.w <= 0.0 {
            continue;
        }
        let label = match b {
            BarButton::Cpu => "CPU".to_string(),
            BarButton::Tempo => "テンポ".to_string(),
            BarButton::Meter => "拍子".to_string(),
            _ => crate::hover_tip::text(s, Hit::Bar(b))
                .and_then(|t| t.split("  ").next().map(str::to_string))
                .unwrap_or_default(),
        };
        if label.is_empty() {
            continue;
        }
        let mut node = accesskit::Node::new(accesskit::Role::Button);
        node.set_label(label);
        let on = match b {
            BarButton::Play => Some(v.playing),
            BarButton::Record => Some(v.recording),
            BarButton::Loop => Some(v.looping),
            BarButton::PreRoll => Some(v.pre_roll_on),
            BarButton::Metronome => Some(v.metronome),
            _ => None,
        };
        if let Some(on) = on {
            node.set_toggled(if on {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
        }
        match b {
            BarButton::Cpu => node.set_value(format!("{}%", v.cpu)),
            BarButton::Tempo => node.set_value(v.bpm.clone()),
            BarButton::Meter => node.set_value(v.meter.clone()),
            _ => {}
        }
        node.set_bounds(bounds(r));
        out.push((1 + i, node));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Section;

    /// UI-300 / UI-301: a screen reader finds the transport in the bottom bar, and reads the
    /// CPU's load as a number.
    #[test]
    fn the_bottom_bar_reads_the_transport_and_the_cpu() {
        let s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        let nodes = bottom_bar_nodes(&s, &g);
        let cpu = find(&nodes, accesskit::Role::Button, "CPU").unwrap();
        assert_eq!(cpu.value(), Some(format!("{}%", s.bar_view().cpu).as_str()));
        let b = cpu.bounds().unwrap();
        assert!(b.y0 >= (g.size.1 + Layout::TOP_BAR_H) as f64, "{b:?}");
        let play = nodes
            .iter()
            .map(|(_, n)| n)
            .find(|n| n.role() == accesskit::Role::Button && n.toggled().is_some())
            .unwrap();
        assert_eq!(play.toggled(), Some(accesskit::Toggled::False));
        assert!(find(&nodes, accesskit::Role::Button, "テンポ").is_some());
        assert!(find(&nodes, accesskit::Role::Label, "位置").is_some());
    }

    /// UI-303: a screen reader finds the GitHub mark by its name, and where it goes.
    #[test]
    fn the_about_panel_names_its_mark_only_button() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        assert!(about_nodes(&s, &g).is_empty());
        s.press(&g, crate::studio::Hit::Bar(crate::topbar::BarButton::Logo));
        let nodes = about_nodes(&s, &g);
        let github = find(&nodes, accesskit::Role::Button, crate::about::GITHUB).unwrap();
        assert_eq!(github.description(), Some("github.com/tekalu1/xirius"));
        let b = github.bounds().unwrap();
        assert!(b.y0 >= Layout::TOP_BAR_H as f64, "{b:?}");
        assert!(find(&nodes, accesskit::Role::Heading, "Xirius").is_some());
        assert!(find(&nodes, accesskit::Role::Button, "LAME 3.100").is_some());
    }

    /// UI-311: the value field at the head of a MIDI clip's menu is read with its unit, which
    /// the field does not draw.
    #[test]
    fn a_clip_menu_field_reads_its_unit() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        let (x, y) = crate::arrange::probe::clip_mid(&s, &g, "Keys", 0);
        assert!(s.scene_context(&g, x, y));
        let nodes: Vec<accesskit::Node> = marks(&s, &g).into_iter().map(|m| m.node).collect();
        assert!(
            nodes.iter().any(|n| n.role() == accesskit::Role::SpinButton
                && n.label() == Some("トランスポーズ 0 半音")),
            "{:?}",
            nodes
                .iter()
                .map(|n| (n.role(), n.label()))
                .collect::<Vec<_>>()
        );
    }

    fn open(section: Section) -> (Studio, Geometry) {
        let mut s = Studio::new(crate::song::Song::empty());
        let g = s.geometry(1440.0, 860.0);
        s.resized(&g);
        s.open_settings(section);
        (s, g)
    }

    fn find<'a>(
        nodes: &'a [(usize, accesskit::Node)],
        role: accesskit::Role,
        label: &str,
    ) -> Option<&'a accesskit::Node> {
        nodes
            .iter()
            .map(|(_, n)| n)
            .find(|n| n.role() == role && n.label() == Some(label))
    }

    /// The window an open settings page used to have no accessible children in (a page
    /// without audio inputs: the MIDI page, or the audio page with no device).
    #[test]
    fn every_page_of_the_settings_has_accessible_nodes() {
        for section in Section::ALL {
            let (s, g) = open(section);
            let nodes = settings_nodes(&s, &g);
            assert!(!nodes.is_empty(), "{section:?}: no nodes");
            assert!(
                find(&nodes, accesskit::Role::Tab, section.label())
                    .is_some_and(|n| n.is_selected() == Some(true)),
                "{section:?}: its tab is not selected"
            );
            assert!(find(&nodes, accesskit::Role::Button, "閉じる").is_some());
            for (_, n) in &nodes {
                if matches!(
                    n.role(),
                    accesskit::Role::Switch
                        | accesskit::Role::ComboBox
                        | accesskit::Role::TextInput
                ) {
                    assert!(
                        n.label().is_some_and(|l| !l.is_empty()),
                        "{section:?}: a {:?} without a name",
                        n.role()
                    );
                }
            }
        }
    }

    #[test]
    fn the_midi_input_switch_reads_with_its_label_and_state() {
        let (s, g) = open(Section::Midi);
        let nodes = settings_nodes(&s, &g);
        let pc = find(&nodes, accesskit::Role::Switch, "PC のキーボード")
            .unwrap_or_else(|| panic!("a switch for the PC keyboard"));
        assert!(pc.toggled().is_some());
    }

    fn sample() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = false;
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    fn track(s: &Studio, name: &str) -> TrackId {
        s.project()
            .tracks
            .values()
            .find(|t| t.name == name)
            .expect("a track")
            .id
    }

    /// `n` takes of two bars on `t` from bar 40 on, where the sample song has nothing; the view
    /// scrolled to them.
    fn record(s: &mut Studio, t: TrackId, n: usize) {
        let start = s.project().time_map().bar_start(40);
        let takes: Vec<_> = (0..n)
            .map(|i| {
                serde_json::json!({
                    "track": t.to_string(), "start": start, "length": 2 * 3840,
                    "file": format!("Recordings/{t}-{i}.wav"),
                })
            })
            .collect();
        let ok = s.command(|song| {
            song.run("record.take.add", serde_json::json!({ "takes": takes }))?;
            Ok(())
        });
        assert!(ok && s.error.is_none(), "{:?}", s.error);
        s.scroll.0 = (s.ticks_to_px(start) - 40.0).max(0.0);
    }

    fn rect(n: &accesskit::Node) -> Rect {
        let b = n.bounds().expect("bounds");
        Rect::new(
            b.x0 as f32,
            b.y0 as f32 - Layout::TOP_BAR_H,
            b.width() as f32,
            b.height() as f32,
        )
    }

    #[test]
    fn ui_298_the_search_box_of_the_instruments_and_the_effects_is_an_edit_named_search() {
        let (mut s, g) = sample();
        let keys = track(&s, "Keys");
        let guitar = track(&s, "Guitar");
        for kind in [
            crate::popup::PopupKind::Instrument(keys),
            crate::popup::PopupKind::Insert(guitar),
        ] {
            s.open_popup(kind, Rect::new(200.0, 200.0, 120.0, 24.0));
            let open = marks(&s, &g);
            assert_eq!(open.len(), 1, "{kind:?}: the search box alone");
            let n = &open[0].node;
            assert_eq!(n.role(), accesskit::Role::TextInput, "{kind:?}");
            assert_eq!(n.label(), Some("検索"), "{kind:?}");
            assert_eq!(n.value(), Some(""), "{kind:?}: nothing typed yet");
            let field = s.popup.as_ref().unwrap().layout(g.size).cells[0].rect;
            assert_eq!(rect(n), field, "{kind:?}: over the box");
            for ch in ["s", "Y"] {
                assert!(s.fx_typed(ch, Some(ch)));
            }
            assert_eq!(marks(&s, &g)[0].node.value(), Some("sY"), "{kind:?}");
            assert!(open[0].fold.is_none());
            s.popup = None;
        }
    }

    #[test]
    fn ui_298_a_clip_s_count_of_takes_is_a_button_that_opens_its_rows() {
        let (mut s, g) = sample();
        let vocal = track(&s, "Vocal");
        let guitar = track(&s, "Guitar");
        // One take shows no count, so it has no button.
        record(&mut s, guitar, 1);
        let row = *s.tree().row_of(RowKind::Track(guitar)).unwrap();
        let start = s.project().time_map().bar_start(40);
        let take = s.project().tracks[&guitar]
            .clips
            .iter()
            .find(|c| c.start == start)
            .cloned()
            .expect("the take");
        let r = s.clip_rect(&g, &row, &take);
        assert!(
            g.lanes.contains(r.x + 8.0, r.bottom() - 8.0),
            "the take is on screen"
        );
        assert!(marks(&s, &g).is_empty());
        record(&mut s, vocal, 3);
        let marks_now = marks(&s, &g);
        assert_eq!(marks_now.len(), 1, "the clip that plays carries the count");
        let m = &marks_now[0];
        assert_eq!(m.node.role(), accesskit::Role::Button);
        assert_eq!(m.node.label(), Some("テイク 3"));
        assert_eq!(m.node.is_expanded(), Some(false), "the rows start folded");
        assert_eq!(m.fold, Some(vocal));
        // It lies over the count the pointer presses.
        let r = rect(&m.node);
        assert!(matches!(
            s.arrange_spot(&g, r.x + r.w / 2.0, r.y + r.h / 2.0),
            crate::arrange::Spot::Rec { track, part: crate::rec_area::RecPart::Fold }
                if track == vocal
        ));
        // Its press opens the rows, and it says so; pressed again it folds them.
        assert!(fold(&mut s, vocal));
        assert!(s.tree().row_of(RowKind::Track(vocal)).unwrap().rec_h > 0.0);
        assert_eq!(marks(&s, &g)[0].node.is_expanded(), Some(true));
        assert!(fold(&mut s, vocal));
        assert_eq!(marks(&s, &g)[0].node.is_expanded(), Some(false));
        // A pop-over takes the presses: the counts under it are not offered.
        s.open_popup(
            crate::popup::PopupKind::Song,
            Rect::new(200.0, 200.0, 120.0, 24.0),
        );
        assert!(marks(&s, &g).is_empty());
        s.popup = None;
        assert_eq!(marks(&s, &g).len(), 1);
        // Scrolled away, it is not offered either.
        s.scroll.0 = 0.0;
        assert!(marks(&s, &g).is_empty());
    }

    #[test]
    fn closed_settings_have_none_and_the_track_list_returns() {
        let mut s = Studio::new(crate::song::Song::empty());
        let g = s.geometry(1440.0, 860.0);
        assert!(settings_nodes(&s, &g).is_empty());
        s.open_settings(Section::General);
        assert!(track_rows(&s, &g).is_empty());
    }
}
