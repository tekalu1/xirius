//! The track list as an accessible tree (UI-026): every row a TreeItem named after the track
//! or folder it shows (folders carrying their folded state and depth), so a screen reader
//! reads the list. Only while the settings are closed — they cover the column.
//! Over the picture, the controls a screen reader could not reach (UI-298): the search box of
//! the effects and instruments lists, and each clip's count of takes.
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tree::RowKind;
use gpui::accesskit;
use xirius_core::id::{PluginId, TrackId};
use xirius_design::Layout;

/// The rows of the track column as accessible nodes (window coordinates).
pub fn track_rows(s: &Studio, g: &Geometry) -> Vec<(usize, accesskit::Node)> {
    if s.settings.is_some() || s.opening.is_some() || s.sound_preparing() {
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
            // Every selected row says so, not just the focus (UI-340).
            node.set_selected(s.is_picked(row.kind));
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
    if s.sound_preparing() {
        return Vec::new();
    }
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
    /// Nested controls, such as the radios in the roll view group.
    pub children: Vec<Mark>,
    pub node: accesskit::Node,
    /// The track whose take rows a press on it opens or folds.
    pub fold: Option<TrackId>,
    /// The track whose instrument a press on it picks (the strip's icon-only row, UI-305).
    pub pick: Option<TrackId>,
    /// What a press on it presses, as the pointer would (the audition's button, BRW-023).
    pub press: Option<crate::studio::Hit>,
    /// The device's control a screen reader's increment, decrement and value set turn
    /// (UI-325, M-ui-2).
    pub turn: Option<(PluginId, usize)>,
    /// The velocity layer's edge of the chosen pad a screen reader's increment, decrement
    /// and value set move (low 3).
    pub edge: Option<(PluginId, usize)>,
}

pub(crate) fn bounds(r: Rect) -> accesskit::Rect {
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
    if s.settings.is_some() || s.opening.is_some() || s.sound_preparing() {
        return Vec::new();
    }
    if s.popup.is_none() && s.home_shown() {
        return home_marks(s, g);
    }
    if let Some(p) = &s.popup {
        if p.preset_save.is_some() {
            use crate::popup::{Action, Look};
            return p
                .layout(g.size)
                .cells
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let role = match c.look {
                        Look::Input => accesskit::Role::TextInput,
                        Look::Check => accesskit::Role::CheckBox,
                        _ if c.live() => accesskit::Role::Button,
                        _ => accesskit::Role::Label,
                    };
                    let mut node = accesskit::Node::new(role);
                    node.set_label(if c.look == Look::Input {
                        if c.action == Some(Action::Ctx(2)) {
                            "新しいフォルダー".to_string()
                        } else {
                            "プリセット名".to_string()
                        }
                    } else {
                        c.text.clone()
                    });
                    if c.look == Look::Input {
                        node.set_value(c.text.clone());
                    }
                    if c.look == Look::Check {
                        node.set_toggled(if c.on {
                            accesskit::Toggled::True
                        } else {
                            accesskit::Toggled::False
                        });
                    }
                    if !c.enabled {
                        node.set_disabled();
                    }
                    node.set_bounds(bounds(c.rect));
                    Mark {
                        key: format!("preset-save-{i}"),
                        children: Vec::new(),
                        node,
                        fold: None,
                        pick: None,
                        press: c.live().then_some(Hit::Item(i)),
                        turn: None,
                        edge: None,
                    }
                })
                .collect();
        }
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
                .filter_map(|(i, c)| {
                    let policy=|item: &crate::context::Item| matches!(&item.run,
                        crate::context::Run::Cmd("prefs.set",args) if args["key"].as_str().is_some_and(|k|k.starts_with("plugins.isolation.")));
                    let process=s.ctx.as_ref().and_then(|ctx|ctx.items.iter().find(|item|item.label==c.text))
                        .and_then(|item| if policy(item) { Some(true) } else if matches!(&item.run,
                            crate::context::Run::Sub(rows) if rows.iter().any(policy)) { Some(false) } else { None });
                    if c.spoken.is_empty() && process.is_none() { return None; }
                    let role=match process {Some(true)=>accesskit::Role::RadioButton,Some(false)=>accesskit::Role::Button,None=>accesskit::Role::SpinButton};
                    let mut node = accesskit::Node::new(role);
                    node.set_label(if process.is_some() {c.text.clone()} else {c.spoken.clone()});
                    if process==Some(true) { node.set_toggled(if c.on {accesskit::Toggled::True} else {accesskit::Toggled::False}); }
                    if !c.live() { node.set_disabled(); }
                    node.set_bounds(bounds(c.rect));
                    Some(Mark {
                        key: format!("field-{i}"),
                        children: Vec::new(),
                        node,
                        fold: None,
                        pick: None,
                        press: (process.is_some() && c.live()).then_some(Hit::Item(i)),
                        turn: None,
                        edge: None,
                    })
                })
                .collect();
        }
        let Some(pick) = &p.pick else {
            if matches!(
                p.kind,
                crate::popup::PopupKind::Plugin(..) | crate::popup::PopupKind::CrashedPlugin(..)
            ) {
                return p
                    .layout(g.size)
                    .cells
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.look == crate::popup::Look::Segment && c.live())
                    .map(|(i, c)| {
                        let mut node = accesskit::Node::new(accesskit::Role::RadioButton);
                        node.set_label(c.text.clone());
                        node.set_toggled(if c.on {
                            accesskit::Toggled::True
                        } else {
                            accesskit::Toggled::False
                        });
                        node.set_bounds(bounds(c.rect));
                        Mark {
                            key: format!("plugin-process-{i}"),
                            children: vec![],
                            node,
                            fold: None,
                            pick: None,
                            press: Some(Hit::Item(i)),
                            turn: None,
                            edge: None,
                        }
                    })
                    .collect();
            }
            return Vec::new();
        };
        let layout = p.layout(g.size);
        return layout
            .cells
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if c.look == crate::popup::Look::Input && c.icon == Some("search") {
                    let mut node = accesskit::Node::new(accesskit::Role::TextInput);
                    node.set_label("検索");
                    node.set_value(pick.query.clone());
                    node.set_bounds(bounds(c.rect));
                    return Some(Mark {
                        key: "search".into(),
                        children: Vec::new(),
                        node,
                        fold: None,
                        pick: None,
                        press: None,
                        turn: None,
                        edge: None,
                    });
                }
                // 「一致する…はありません」 as a status (UI-320).
                if c.look == crate::popup::Look::Status {
                    let mut node = accesskit::Node::new(accesskit::Role::Status);
                    node.set_label(c.text.clone());
                    node.set_bounds(bounds(c.rect));
                    return Some(Mark {
                        key: format!("status-{i}"),
                        children: Vec::new(),
                        node,
                        fold: None,
                        pick: None,
                        press: None,
                        turn: None,
                        edge: None,
                    });
                }
                if matches!(
                    p.kind,
                    crate::popup::PopupKind::InstMenu(crate::instrument::menu::Menu::Row(..))
                ) && matches!(&c.action,Some(crate::popup::Action::Pick(to)) if to=="process")
                {
                    let mut node = accesskit::Node::new(accesskit::Role::Button);
                    node.set_label(c.text.clone());
                    node.set_bounds(bounds(c.rect));
                    return Some(Mark {
                        key: "instrument-process".into(),
                        children: vec![],
                        node,
                        fold: None,
                        pick: None,
                        press: Some(Hit::Item(i)),
                        turn: None,
                        edge: None,
                    });
                }
                if let crate::popup::Look::Key(look) = c.look {
                    return key_source_mark(i, c, look);
                }
                None
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
        // The head's volume fader (a Slider in dB); a press on it opens the number field.
        // Whether it is on show is known before its value is read: a narrow column, or a row
        // scrolled out, costs nothing (the value is a track's lanes at the playhead).
        if let Some(r) = s
            .head_fader_row_rect(g, row)
            .filter(|r| g.column.contains(r.x + r.w / 2.0, r.y + r.h / 2.0))
            && let Some(db) = s.head_fader_value(t)
        {
            let mut node = accesskit::Node::new(accesskit::Role::Slider);
            node.set_label(format!("{} の音量", track.name));
            node.set_min_numeric_value(f64::from(crate::mixer::meters::FLOOR_DB));
            node.set_max_numeric_value(f64::from(crate::mixer::meters::CEIL_DB));
            node.set_numeric_value(db.max(f64::from(crate::mixer::meters::FLOOR_DB)));
            node.set_value(format!("{} dB", crate::head_fader::text(db)));
            node.set_bounds(bounds(r));
            out.push(Mark {
                key: format!("head-fader-{t}"),
                children: Vec::new(),
                node,
                fold: None,
                pick: None,
                press: Some(Hit::HeadFader(t)),
                turn: None,
                edge: None,
            });
        }
        let stacks = xirius_core::takes::stacks(track);
        for clip in &track.clips {
            // The ARA badge: a button with the plugin's state, and why it cannot be opened.
            if let (Some(badge), Some(at)) = (s.ara_badge(clip.id), s.ara_badge_at(g, t, clip.id))
                && g.lanes.contains(at.x + at.w / 2.0, at.y + at.h / 2.0)
            {
                let name = s.ara_name(clip.id).unwrap_or_default();
                let mut node = accesskit::Node::new(accesskit::Role::Button);
                node.set_label(format!("{name}: {}", badge.said()));
                if let Some(why) = badge.reason() {
                    node.set_description(why.to_string());
                }
                node.set_bounds(bounds(at));
                out.push(Mark {
                    key: format!("ara-{}", clip.id),
                    children: Vec::new(),
                    node,
                    fold: None,
                    pick: None,
                    press: Some(Hit::Ara(t, clip.id)),
                    turn: None,
                    edge: None,
                });
            }
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
                children: Vec::new(),
                node,
                fold: Some(t),
                pick: None,
                press: None,
                turn: None,
                edge: None,
            });
        }
    }
    out.extend(start_marks(s, g));
    out.extend(inspector_marks(s, g));
    out.extend(instrument_marks(s, g));
    out.extend(crate::channel::marks(s, g));
    out.extend(crate::macro_pad::marks(s));
    out.extend(flow_marks(s, g));
    out.extend(device_marks(s, g));
    out.extend(audition_mark(s, g));
    out.extend(preset_marks(s, g));
    out.extend(file_marks(s, g));
    out.extend(drawer_marks(s, g));
    out.extend(loop_marks(s, g));
    out
}

/// What a screen reader calls the loop band: 「ループ 5 小節〜8 小節 オン」, the bars it starts
/// and ends in and whether it repeats (2026-11-05).
pub fn loop_name(s: &Studio) -> Option<String> {
    let p = s.project();
    let l = p.loop_range?;
    let tm = p.time_map();
    let (a, b) = (tm.to_bbt(l.start).0, tm.to_bbt((l.end - 1).max(l.start)).0);
    let on = if s.transport.looping {
        "オン"
    } else {
        "オフ"
    };
    Some(format!("ループ {a} 小節〜{b} 小節 {on}"))
}

/// The loop band on the arrangement's ruler and on the roll's: a Button named by
/// [`loop_name`], toggled while the loop repeats; a press turns the loop on or off, as a press
/// let go on the band does (2026-11-05).
pub fn loop_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::topbar::BarButton;
    let (Some(name), Some(l)) = (loop_name(s), s.project().loop_range) else {
        return Vec::new();
    };
    let mark = |key: &str, ruler: Rect, scale: crate::ruler::Scale, view: (f32, f32)| {
        let (x0, x1) = (scale.x(l.start).max(view.0), scale.x(l.end).min(view.1));
        if x1 <= x0 || ruler.w <= 0.0 {
            return None;
        }
        let mut node = accesskit::Node::new(accesskit::Role::Button);
        node.set_label(name.clone());
        node.set_toggled(if s.transport.looping {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        });
        node.set_bounds(bounds(Rect::new(
            x0,
            ruler.y,
            x1 - x0,
            crate::ruler::LOOP_H,
        )));
        Some(Mark {
            key: key.into(),
            children: Vec::new(),
            node,
            fold: None,
            pick: None,
            press: Some(Hit::Bar(BarButton::Loop)),
            turn: None,
            edge: None,
        })
    };
    let mut out = Vec::new();
    if s.popup.is_none() && g.ruler.w > 0.0 && g.ruler.h > 0.0 {
        out.extend(mark(
            "loop-band",
            g.ruler,
            s.ruler_scale(g),
            (g.lanes.x, g.lanes.right()),
        ));
    }
    // Under a menu neither band is offered, as the menu takes the presses.
    if s.popup.is_none()
        && let Some(ctx) = s.roll_ctx(g)
    {
        let f = s.roll.frame(ctx.body);
        out.extend(mark(
            "roll-loop-band",
            f.ruler,
            s.roll.ruler_scale(&ctx),
            (f.grid.x, f.grid.right()),
        ));
    }
    out
}

/// The new song's screen: the entries that are pressed as Buttons (the one to record says
/// 「入力なし」 when there is nothing to record from), 「ファイルをドロップ」 as a label, and
/// 「オーディオデバイスなし」 with its menu when there is no device (UI-350).
pub fn start_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::start::{Entry, Part};
    use crate::studio::Hit;
    if !s.start_shown() || s.home_shown() {
        return Vec::new();
    }
    let l = s.start_layout(g);
    let mark = |key: &str, role, label: &str, r, press: Option<Hit>| {
        let mut node = accesskit::Node::new(role);
        node.set_label(label);
        node.set_bounds(bounds(r));
        Mark {
            key: key.into(),
            children: Vec::new(),
            node,
            fold: None,
            pick: None,
            press,
            turn: None,
            edge: None,
        }
    };
    let mut out = Vec::new();
    for e in Entry::ALL {
        let (r, _) = l.entry(e);
        let mut m = if e.pressable() {
            mark(
                &format!("start-{e:?}"),
                accesskit::Role::Button,
                e.label(),
                r,
                Some(Hit::Start(e)),
            )
        } else {
            mark(
                &format!("start-{e:?}"),
                accesskit::Role::Label,
                e.label(),
                r,
                None,
            )
        };
        if e == Entry::Audio && s.audio_entry_marked() {
            m.node.set_description(crate::input_row::NO_INPUT);
        }
        out.push(m);
    }
    if let Some(r) = l.no_device {
        let mut m = mark(
            "start-no-device",
            accesskit::Role::Button,
            crate::start::rig::NO_DEVICE,
            r,
            Some(Hit::Entrance(Part::NoDevice)),
        );
        m.node.set_has_popup(accesskit::HasPopup::Menu);
        out.push(m);
    }
    out
}

/// The start screen (UI-350): its three buttons, 「オーディオデバイスなし」, the search box
/// 「最近の曲を検索」 and the list 「最近の曲」 with its rows, the chosen one selected.
pub fn home_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::home::{Button, Focus, Spot};
    use crate::studio::Hit;
    let (Some(home), Some(l)) = (s.home.as_ref(), s.home_layout(g)) else {
        return Vec::new();
    };
    let mark = |key: String, role, label: String, r, press: Option<Spot>| {
        let mut node = accesskit::Node::new(role);
        node.set_label(label);
        node.set_bounds(bounds(r));
        Mark {
            key,
            children: Vec::new(),
            node,
            fold: None,
            pick: None,
            press: press.map(Hit::Home),
            turn: None,
            edge: None,
        }
    };
    let mut out = Vec::new();
    for b in Button::ALL {
        let label = match b.keys() {
            "" => b.label().to_string(),
            keys => format!("{} {keys}", b.label()),
        };
        let mut m = mark(
            format!("home-{b:?}"),
            accesskit::Role::Button,
            label,
            l.button(b),
            Some(Spot::Button(b)),
        );
        if home.focus == Focus::Button(b) {
            m.node.set_selected(true);
        }
        out.push(m);
    }
    if let Some(r) = l.no_device {
        let mut m = mark(
            "home-no-device".into(),
            accesskit::Role::Button,
            crate::start::rig::NO_DEVICE.into(),
            r,
            Some(Spot::Device),
        );
        m.node.set_has_popup(accesskit::HasPopup::Menu);
        out.push(m);
    }
    if let Some(r) = l.search {
        let mut m = mark(
            "home-search".into(),
            accesskit::Role::TextInput,
            crate::home::SEARCH_NAME.into(),
            r,
            Some(Spot::Search),
        );
        m.node.set_value(home.pick.query.clone());
        out.push(m);
    }
    let rows = home.pick.nav();
    let mut list = mark(
        "home-list".into(),
        accesskit::Role::List,
        crate::home::RECENT.into(),
        l.list(),
        None,
    );
    for (i, r) in &l.rows {
        let Some(row) = rows.get(*i) else {
            continue;
        };
        let mut label = row.label.clone();
        if matches!(row.act, Some(crate::popup::Action::OpenSample)) {
            label = format!("{label} {}", crate::home::SAMPLE_TAG);
        }
        let mut item = mark(
            format!("home-row-{i}"),
            accesskit::Role::ListItem,
            label,
            *r,
            Some(Spot::Row(*i)),
        );
        item.node.set_selected(*i == home.pick.selected);
        list.children.push(item);
    }
    out.push(list);
    out
}

/// The icon-only drawer controls, with stable names and their current state (UI-343).
pub fn drawer_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::studio::{DrawerTab, Hit};
    use crate::topbar::BarButton;
    if s.settings.is_some() || s.popup.is_some() {
        return Vec::new();
    }
    let control = |key: &str, role, label: String, r, press| {
        let mut node = accesskit::Node::new(role);
        node.set_label(label);
        node.set_bounds(bounds(r));
        Mark {
            key: key.into(),
            children: Vec::new(),
            node,
            fold: None,
            pick: None,
            press,
            turn: None,
            edge: None,
        }
    };
    let mut out = Vec::new();
    for tab in DrawerTab::ALL {
        let mut m = control(
            &format!("drawer-{tab:?}"),
            accesskit::Role::Tab,
            tab.label().into(),
            g.tab(tab),
            Some(Hit::Tab(tab)),
        );
        m.node.set_selected(s.drawer_open && s.tab == tab);
        out.push(m);
    }
    if !s.roll_switch_shown() {
        return out;
    }
    let drums = s.roll_drum();
    let keys = g.roll_switch(false);
    let mut group = control(
        "roll-view",
        accesskit::Role::RadioGroup,
        "見方".into(),
        Rect::new(keys.x - 2.0, keys.y, 60.0, keys.h),
        None,
    );
    for (on, label) in [(false, "鍵盤"), (true, "ドラム")] {
        let mut m = control(
            &format!("roll-view-{on}"),
            accesskit::Role::RadioButton,
            label.into(),
            g.roll_switch(on),
            Some(Hit::RollView(on)),
        );
        m.node.set_toggled(if on == drums.is_some() {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        });
        group.children.push(m);
    }
    out.push(group);
    if let Some(map) = drums {
        out.push(control(
            "roll-map",
            accesskit::Role::Button,
            format!("ドラムのマップ: {}", xirius_core::drum::map_label(&map)),
            g.roll_map(),
            Some(Hit::RollMap),
        ));
    } else {
        let on = s.draw_length_last();
        let mut m = control(
            "roll-length",
            accesskit::Role::Button,
            "直前のノートの長さで入力".into(),
            g.roll_length(),
            Some(Hit::RollLength(!on)),
        );
        // AccessKit maps a Button with toggled state to a toggle button.
        m.node.set_toggled(if on {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        });
        out.push(m);
        if let Some(text) = s.draw_length_text() {
            let r = s.bar_layout(g.size.0).length;
            // The top bar is already in window coordinates, unlike drawer geometry.
            let mut m = control(
                "draw-length-chip",
                accesskit::Role::Button,
                format!("グリッドの長さで入力: {text}"),
                r,
                Some(Hit::Bar(BarButton::DrawLength)),
            );
            m.node.set_bounds(accesskit::Rect::new(
                r.x as f64,
                r.y as f64,
                r.right() as f64,
                r.bottom() as f64,
            ));
            out.push(m);
        }
    }
    out
}

/// The browser's audition button draws only a mark, so it is a Button named for what a press
/// does: 「ファイルを試聴」, 「停止」 while the file plays, and 「読み込んでいます」 while nothing is
/// heard yet (BRW-023). Its press is the pointer's.
fn audition_mark(s: &Studio, g: &Geometry) -> Option<Mark> {
    if !s.browser.open {
        return None;
    }
    let l = s.browser.layout(g.panel);
    let (r, playing, key) = l.rows.iter().find_map(|p| match &p.row {
        crate::browser::BRow::Audition {
            playing,
            depth,
            key,
        } => Some((
            s.browser
                .audition_parts(s.browser.row_rect(&l, p), *depth)
                .0,
            *playing,
            key.as_str(),
        )),
        _ => None,
    })?;
    let label = if !playing {
        crate::audition::PLAY_LABEL
    } else if crate::audition::waiting(key) {
        crate::audition::READING
    } else {
        crate::audition::STOP_LABEL
    };
    let mut node = accesskit::Node::new(accesskit::Role::Button);
    node.set_label(label);
    node.set_bounds(bounds(r));
    Some(Mark {
        key: "audition".into(),
        children: Vec::new(),
        node,
        fold: None,
        pick: None,
        press: Some(crate::studio::Hit::Browser(
            crate::browser::BrowserHit::Audition,
        )),
        turn: None,
        edge: None,
    })
}

/// File labels keep the full name even when the painter fits it with an ellipsis.
fn file_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::browser::{BRow, BrowserHit, RailTab};
    if !s.browser.open || s.browser.tab != RailTab::Files {
        return Vec::new();
    }
    let l = s.browser.layout(g.panel);
    l.rows
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let BRow::File {
                key,
                name,
                len,
                selected,
                ..
            } = &p.row
            else {
                return None;
            };
            let r = s.browser.row_rect(&l, p);
            if r.bottom() <= l.list.y || r.y >= l.list.bottom() {
                return None;
            }
            let mut node = accesskit::Node::new(accesskit::Role::ListItem);
            node.set_label(name.clone());
            node.set_description(len.clone());
            node.set_selected(*selected);
            node.set_bounds(bounds(r));
            Some(Mark {
                key: format!("browser-file-{key}"),
                children: Vec::new(),
                node,
                fold: None,
                pick: None,
                press: Some(crate::studio::Hit::Browser(BrowserHit::File(i))),
                turn: None,
                edge: None,
            })
        })
        .collect()
}

/// The presets under the built-in devices in the browser (BRW-017): the arrow before a device
/// is a Button 「EQ のプリセット」 that says whether they show, and each preset row a ListItem
/// 「EQ のプリセット Hall」 that says whether it is chosen. A press is the pointer's (a row is
/// chosen and Enter loads it).
fn preset_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::browser::{BRow, BrowserHit, RailTab};
    if !s.browser.open || !matches!(s.browser.tab, RailTab::Instruments | RailTab::Effects) {
        return Vec::new();
    }
    let l = s.browser.layout(g.panel);
    let mut out = Vec::new();
    for (i, r) in l.tabs.iter().enumerate() {
        let mut node = accesskit::Node::new(accesskit::Role::Button);
        node.set_label(["インストゥルメント", "FX", "トラック"][i]);
        node.set_bounds(bounds(*r));
        out.push(Mark {
            key: format!("track-browser-kind-{i}"),
            children: Vec::new(),
            node,
            fold: None,
            pick: None,
            press: Some(Hit::Browser(BrowserHit::Loc(i))),
            turn: None,
            edge: None,
        });
    }
    for (i, p) in l.rows.iter().enumerate() {
        let r = s.browser.row_rect(&l, p);
        if r.bottom() <= l.list.y || r.y >= l.list.bottom() {
            continue;
        }
        let (key, role, label, press) = match &p.row {
            BRow::Group {
                key, name, open, ..
            } if s.browser.track_library.is_some() => {
                let mut node = accesskit::Node::new(accesskit::Role::TreeItem);
                node.set_expanded(*open);
                (
                    format!("track-browser-{key}"),
                    node,
                    name.to_string(),
                    BrowserHit::Group(i),
                )
            }
            BRow::Item {
                item,
                presets: None,
                ..
            } if s.browser.track_library.is_some() => (
                format!("track-browser-device-{}", item.name),
                accesskit::Node::new(accesskit::Role::ListItem),
                item.name.to_string(),
                BrowserHit::Item(i),
            ),
            BRow::Item {
                item,
                presets: Some(open),
                ..
            } => {
                let mut node = accesskit::Node::new(accesskit::Role::Button);
                node.set_expanded(*open);
                (
                    format!("presets-{}", item.name),
                    node,
                    format!("{} のプリセット", item.name),
                    BrowserHit::Disclose(i),
                )
            }
            BRow::Preset {
                device,
                name,
                selected,
                ..
            } => {
                let mut node = accesskit::Node::new(accesskit::Role::ListItem);
                node.set_selected(*selected);
                let device_name = s
                    .browser
                    .track_library
                    .as_ref()
                    .and_then(|es| es.iter().find(|e| e.id == *device))
                    .map(|e| e.device_name.as_str())
                    .unwrap_or_else(|| xirius_params::find(device).map_or(*device, |d| d.name));
                (
                    format!("preset-{device}-{name}"),
                    node,
                    format!("{device_name} のプリセット {name}"),
                    BrowserHit::Preset(i),
                )
            }
            _ => continue,
        };
        let mut node = role;
        node.set_label(label);
        let at = match press {
            BrowserHit::Disclose(_) => s.browser.disclose_rect(r),
            _ => r,
        };
        node.set_bounds(bounds(at));
        out.push(Mark {
            key,
            children: Vec::new(),
            node,
            fold: None,
            pick: None,
            press: Some(crate::studio::Hit::Browser(press)),
            turn: None,
            edge: None,
        });
    }
    out
}

/// A screen reader's press on a mark that presses as the pointer does.
pub fn press(s: &mut Studio, g: &Geometry, hit: crate::studio::Hit) -> bool {
    s.press(g, hit)
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
                children: Vec::new(),
                node,
                fold: None,
                pick: None,
                press: None,
                turn: None,
                edge: None,
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
                children: Vec::new(),
                node,
                fold: None,
                pick: Some(strip.track),
                press: None,
                turn: None,
                edge: None,
            })
        })
        .collect()
}

/// The parts of a strip's flow that show only an icon (MIX-044): a splitter's chevron is a
/// Button named 「スプリッター」 that says whether its branches are open, and the ＋ of each
/// area a Button named for what it adds. A row's M is a toggle Button named for the branch or
/// layer it mutes, the ＋ after a row a Button 「<row> の後ろに挿す」 and the bar of the level
/// row a Slider in dB; the buttons by a strip's name are 「チャンネルエディターを開く」 and
/// 「広げる」 (a toggle) (MIX-045). A key send's row is a Button 「サイドチェーンのセンド：
/// <strip> › <plug-in>」 that opens the receiver, its bar a Slider of the level in dB (MIX-048).
/// Only a part on screen has one.
fn flow_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::mixer::MixerHit;
    use crate::mixer::flow::{Area, Kind};
    use xirius_core::commands::labels::label;
    let p = s.project();
    let mut out = Vec::new();
    for strip in s.mixer_strips(g) {
        let Some(track) = p.tracks.get(&strip.track) else {
            continue;
        };
        let t = strip.track;
        let flow = s.strip_flow(&strip, track);
        let mut parts: Vec<(MixerHit, accesskit::Node)> = Vec::new();
        let button = |name: &str| {
            let mut node = accesskit::Node::new(accesskit::Role::Button);
            node.set_label(name);
            node
        };
        let toggled = |node: &mut accesskit::Node, on: bool| {
            node.set_toggled(if on {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
        };
        let mut editor = button(crate::mixer::EDITOR_LABEL);
        editor.set_expanded(s.channel.as_ref().is_some_and(|w| w.track == t));
        parts.push((MixerHit::Editor(t), editor));
        let mut widen = button(crate::mixer::WIDEN_LABEL);
        toggled(&mut widen, s.mixer.wide.contains(&t));
        parts.push((MixerHit::Widen(t), widen));
        let mut splits = 0;
        for (l, _) in &flow.rows {
            match (l.kind, l.area) {
                (Kind::Split { id, open, .. }, _) => {
                    // each its own name: the first, the second, … from the top
                    splits += 1;
                    let mut node = button(&format!("スプリッター {splits}"));
                    node.set_expanded(open);
                    parts.push((MixerHit::Fold(t, id), node));
                    // its ＋ (not on a channel split; at 116 only while shown: MIX-045)
                    parts.push((MixerHit::AddBranch(t, id), button("分岐を追加")));
                }
                (Kind::Head { add }, Area::Insert) => {
                    // the heading is dragged and copied from (MIX-045); its ＋ adds (MIX-044)
                    let head = button(crate::mixer::INSERT_HEAD);
                    parts.push((MixerHit::Heading(t, Area::Insert), head));
                    if add {
                        let plus = button(crate::mixer::ADD_EFFECT_LABEL);
                        parts.push((MixerHit::AddInsert(t), plus));
                    }
                }
                (Kind::Head { add }, Area::Instrument) => {
                    // the heading names the area its ＋ adds to
                    let mut head = accesskit::Node::new(accesskit::Role::Label);
                    head.set_label(crate::mixer::INSTRUMENT_HEAD);
                    parts.push((MixerHit::Heading(t, Area::Instrument), head));
                    if add && flow.has_instrument() {
                        let name = label("rack.layer.add").unwrap_or("rack.layer.add");
                        parts.push((MixerHit::AddLayer(t), button(name)));
                    } else if add {
                        let plus = button(crate::mixer::ADD_INSTRUMENT_LABEL);
                        parts.push((MixerHit::AddInstrument(t), plus));
                    }
                }
                _ => {}
            }
            // the level row speaks for the row it is under
            let row = if l.kind == Kind::Level {
                s.mixer
                    .chosen
                    .and_then(|(_, pick)| flow.picked(pick))
                    .map(|(l, _)| *l)
            } else {
                Some(*l)
            };
            let Some(row) = row else {
                continue;
            };
            let name = crate::mixer::flow::row_name(track, &row);
            if let Some(target) = l.target {
                let whose = if target.layer {
                    "レイヤー"
                } else {
                    "分岐"
                };
                // the level row's mute is the chosen row's, said once
                if l.kind != Kind::Level {
                    let mut mute = button(&format!("{name} の{whose}をミュート"));
                    toggled(&mut mute, target.mute);
                    parts.push((MixerHit::RowMute(t, target.id), mute));
                }
                if l.kind == Kind::Level {
                    let mut node = accesskit::Node::new(accesskit::Role::Slider);
                    node.set_label(format!("{name} の{whose}の音量"));
                    let db = s
                        .mixer
                        .level_drag
                        .filter(|d| d.track == t && d.node == target.id)
                        .map_or(target.db, |d| d.value);
                    node.set_min_numeric_value(f64::from(crate::mixer::meters::FLOOR_DB));
                    node.set_max_numeric_value(f64::from(crate::mixer::meters::CEIL_DB));
                    node.set_numeric_value(db);
                    node.set_value(crate::mixer::flow::level_text((db, false)));
                    parts.push((MixerHit::Level(t, target.id), node));
                    if !target.layer {
                        let mut node = accesskit::Node::new(accesskit::Role::Slider);
                        node.set_label(format!("{name} の{whose}のパン"));
                        node.set_min_numeric_value(-100.0);
                        node.set_max_numeric_value(100.0);
                        node.set_numeric_value((target.pan * 100.0).round());
                        node.set_value(crate::branch_value::pan_text(target.pan));
                        parts.push((MixerHit::BranchPan(t, target.id), node));
                    }
                }
            }
            if let Some(pick) = l.pick() {
                parts.push((
                    MixerHit::After(t, pick),
                    button(&format!("{name} の後ろに挿す")),
                ));
            }
        }
        for key in crate::mixer::keys::key_sends(&p, t) {
            let whom = crate::mixer::keys::key_label(&p, &key);
            parts.push((
                MixerHit::KeySend(t, key.id),
                button(&format!("サイドチェーンのセンド：{whom}")),
            ));
            let mut node = accesskit::Node::new(accesskit::Role::Slider);
            node.set_label(format!("{whom} へのセンドの量"));
            let db = s.key_level(t, key.id).unwrap_or(key.level_db);
            node.set_min_numeric_value(xirius_core::model::KEY_MIN_DB);
            node.set_max_numeric_value(xirius_core::model::KEY_MAX_DB);
            node.set_numeric_value(db);
            node.set_value(crate::mixer::keys::level_words(db));
            parts.push((MixerHit::KeyLevel(t, key.id), node));
        }
        for (hit, mut node) in parts {
            let Some(r) = s.mixer_seen(g, hit) else {
                continue;
            };
            node.set_bounds(bounds(r));
            out.push(Mark {
                key: format!("flow-{t}-{hit:?}"),
                children: Vec::new(),
                node,
                fold: None,
                pick: None,
                press: Some(crate::studio::Hit::Mixer(hit)),
                turn: None,
                edge: None,
            });
        }
    }
    out
}

/// A part of a receiver's key sources (UI-336): a source's row is a Group named by its channel,
/// its boxes CheckBoxes 「<channel> センド」「<channel> 出力」, the position a toggle Button
/// 「<channel> プリフェーダー」, the send's amount a Slider in dB, and the loop mark an Image
/// 「ループ」.
fn key_source_mark(
    i: usize,
    c: &crate::popup::Cell,
    look: crate::key_sources::KLook,
) -> Option<Mark> {
    use crate::key_sources::KLook;
    let toggled = |on: bool| {
        if on {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        }
    };
    let mut node = match look {
        KLook::Row { .. } => {
            let mut n = accesskit::Node::new(accesskit::Role::Group);
            n.set_label(c.text.clone());
            n
        }
        KLook::Check { on } => {
            let mut n = accesskit::Node::new(accesskit::Role::CheckBox);
            n.set_toggled(toggled(on));
            n
        }
        KLook::Pre { on } => {
            let mut n = accesskit::Node::new(accesskit::Role::Button);
            n.set_toggled(toggled(on));
            n
        }
        KLook::Bar { .. } => {
            let mut n = accesskit::Node::new(accesskit::Role::Slider);
            let db = c.text.trim_end_matches(" dB").parse::<f64>().unwrap_or(0.0);
            n.set_min_numeric_value(xirius_core::model::KEY_MIN_DB);
            n.set_max_numeric_value(xirius_core::model::KEY_MAX_DB);
            n.set_numeric_value(db);
            n.set_value(c.text.clone());
            n
        }
        KLook::Loop => accesskit::Node::new(accesskit::Role::Image),
        KLook::Column | KLook::Value => return None,
    };
    if !c.spoken.is_empty() {
        node.set_label(c.spoken.clone());
    }
    if !c.enabled {
        node.set_disabled();
    }
    node.set_bounds(bounds(c.rect));
    Some(Mark {
        key: format!("key-{i}"),
        children: Vec::new(),
        node,
        fold: None,
        pick: None,
        press: c.live().then_some(crate::studio::Hit::Item(i)),
        turn: None,
        edge: None,
    })
}

/// The controls of the open device windows (UI-324): a knob or a row of
/// segments is a Slider named as the window names it, with the value it reads;
/// an on and off button is a toggle Button, a choice shown as its name a
/// ComboBox. An EQ's band tabs are Tabs and its analyser's button a ComboBox
/// (UI-326).
fn device_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::device::control::{self, Look};
    use crate::instrument::window::{Part, live, source_of, values_of};
    let p = s.auto_shown();
    let mut out = Vec::new();
    for (i, w) in s.instruments.open.iter().enumerate() {
        let Some((track, x, def)) = live(&p, w) else {
            continue;
        };
        let now = values_of(x, def);
        let r = s.instruments.frame(i, g);
        // a window this surface does not show has no frame here
        if r.w <= 0.0 {
            continue;
        }
        let body = s.instruments.body(r, x, def);
        let mark = |key: String, node: accesskit::Node| Mark {
            key,
            children: Vec::new(),
            node,
            fold: None,
            pick: None,
            press: None,
            turn: None,
            edge: None,
        };
        // The sidechain's seat (UI-336): its on button, and the button that lists the sources
        // (expanded while the list is open).
        let seat = s
            .instruments
            .part_rect(r, &p, track, x, def, Part::Sidechain);
        if seat.w > 0.0 {
            let on = x.sidechain.as_ref().is_some_and(|sc| sc.on);
            let mut node = accesskit::Node::new(accesskit::Role::Button);
            node.set_label("サイドチェーン");
            node.set_toggled(if on {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
            node.set_bounds(bounds(seat));
            out.push(mark(format!("device-{}-sidechain", w.plugin), node));
            let list = s.instruments.part_rect(r, &p, track, x, def, Part::Sources);
            let name = source_of(&p, x);
            let mut node = accesskit::Node::new(accesskit::Role::Button);
            node.set_label(if name.is_empty() {
                "サイドチェーンのソース".to_string()
            } else {
                format!("サイドチェーンのソース {name}")
            });
            let open = s
                .popup
                .as_ref()
                .is_some_and(|p| p.kind == crate::popup::PopupKind::KeySources(w.plugin));
            node.set_expanded(open);
            node.set_bounds(bounds(list));
            out.push(mark(format!("device-{}-sources", w.plugin), node));
        }
        for (t, tab) in body.tabs.iter().enumerate() {
            let mut node = accesskit::Node::new(accesskit::Role::Tab);
            node.set_label(tab.text);
            node.set_selected(tab.chosen);
            node.set_bounds(bounds(tab.r));
            out.push(mark(format!("device-{}-tab-{t}", w.plugin), node));
        }
        if let Some(at) = body.spectrum {
            let tap = s.instruments.tap(w.plugin);
            let mut node = accesskit::Node::new(accesskit::Role::ComboBox);
            node.set_label("Spectrum");
            node.set_value(crate::device::eq::SPECTRUM[tap as usize]);
            node.set_bounds(bounds(crate::device::eq::spectrum_rect(at, tap)));
            out.push(mark(format!("device-{}-spectrum", w.plugin), node));
        }
        if !body.levels.is_empty() {
            use crate::device::dynamics::{Dynamics, Kind};
            let none = Dynamics::default();
            let d = s.instruments.dynamics.get(&w.plugin).unwrap_or(&none);
            for (n, l) in body.levels.iter().enumerate() {
                let mut node = accesskit::Node::new(accesskit::Role::Button);
                node.set_label(match l.kind {
                    Kind::In => "Input peak",
                    Kind::Gr => "Reduction peak",
                    Kind::Out => "Output peak",
                });
                node.set_value(l.text(d));
                node.set_bounds(bounds(l.readout));
                out.push(mark(format!("device-{}-readout-{n}", w.plugin), node));
            }
        }
        // A Sampler's keys and a Drum Kit's pads are Buttons named by their note (PLG-066).
        for k in &body.keys {
            let mut node = accesskit::Node::new(accesskit::Role::Button);
            node.set_label(crate::roll::pitch_name(k.pitch));
            node.set_bounds(bounds(k.r));
            out.push(mark(format!("device-{}-key-{}", w.plugin, k.pitch), node));
        }
        // A pad says its sample too, and the chosen one is selected (L2-smp-6).
        for k in &body.pads {
            use crate::device::layout::Mark as Spot;
            let p = crate::device::play::pad_of(k.pitch);
            let spot = body.spots.iter().find(|s| s.mark == Spot::Pad(p));
            let mut node = accesskit::Node::new(accesskit::Role::Button);
            let mut label = format!(
                "{} {}",
                crate::device::play::pad_name(k.pitch),
                crate::roll::pitch_name(k.pitch)
            );
            if let Some(s) = spot.filter(|s| !s.text.is_empty()) {
                label = format!("{label}, {}", s.text);
            }
            node.set_label(label);
            if spot.is_some_and(|s| s.on) {
                node.set_selected(true);
            }
            node.set_bounds(bounds(k.r));
            out.push(mark(format!("device-{}-pad-{}", w.plugin, k.pitch), node));
        }
        // A Sampler's and a Drum Kit's zones, banks, layers, M and S, the arrows of the
        // sample and Browse are Buttons; a chosen one is selected, an M or S on is toggled
        // (UI-333, UI-334).
        for (n, spot) in body.spots.iter().enumerate() {
            use crate::device::layout::Mark as Spot;
            // The chosen pad's title: its name, sample and note, read as text (L2-smp-6).
            if spot.mark == Spot::Title {
                let mut node = accesskit::Node::new(accesskit::Role::Label);
                let mut label = spot.text.clone();
                if !spot.sub.is_empty() {
                    label = format!("{label}, {}", spot.sub);
                }
                if let Some(note) = spot.note {
                    label = format!("{label}, {}", crate::roll::pitch_name(note));
                }
                node.set_label(label);
                node.set_bounds(bounds(spot.r));
                out.push(mark(format!("device-{}-title", w.plugin), node));
                continue;
            }
            if !spot.mark.takes() {
                continue;
            }
            // A layer's edge is a Slider of the velocity its layer ends at (low 3).
            if let Spot::Edge(j) = spot.mark {
                let files = crate::device::sampler::pad_files(
                    def,
                    x,
                    w.pick.min(crate::device::play::PADS - 1),
                );
                let Some(&(_, hi)) = crate::device::sampler::velocities(def, x, &files).get(j)
                else {
                    continue;
                };
                let (lo, max) = crate::device::sampler::edge_range(def, x, &files, j);
                let mut node = accesskit::Node::new(accesskit::Role::Slider);
                node.set_label(format!("Layer {} top", j + 1));
                node.set_value(spot.text.clone());
                node.set_numeric_value(f64::from(hi));
                node.set_min_numeric_value(f64::from(lo));
                node.set_max_numeric_value(f64::from(max));
                node.set_numeric_value_step(1.0);
                node.set_bounds(bounds(spot.r));
                out.push(Mark {
                    edge: Some((w.plugin, j)),
                    ..mark(format!("device-{}-spot-{n}", w.plugin), node)
                });
                continue;
            }
            let mut node = accesskit::Node::new(accesskit::Role::Button);
            node.set_label(match spot.mark {
                Spot::Prev => "Previous sample".to_string(),
                Spot::Next => "Next sample".to_string(),
                Spot::Mute(p) => format!("Mute {}", xirius_params::PAD_NAMES[p]),
                Spot::Solo(p) => format!("Solo {}", xirius_params::PAD_NAMES[p]),
                Spot::Bank(_) => format!("Bank {}", spot.text),
                Spot::Layer(_) => format!("Layer {}", spot.text),
                Spot::Zone(_) => match spot.note {
                    Some(note) => format!("{}, root {}", spot.text, crate::roll::pitch_name(note)),
                    None => spot.text.clone(),
                },
                _ => spot.text.clone(),
            });
            match spot.mark {
                Spot::Mute(_) | Spot::Solo(_) => node.set_toggled(if spot.on {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                }),
                _ if spot.on => node.set_selected(true),
                _ => {}
            }
            node.set_bounds(bounds(spot.r));
            out.push(mark(format!("device-{}-spot-{n}", w.plugin), node));
        }
        // A graph's point is a Slider for each value it moves that no knob on show has: one
        // Slider a value, named by the point and the value and reading that value alone, keyed
        // by the value, not by where it comes in the list (UI-325, L2-ui-3, low 16).
        let mut taken: Vec<usize> = body.slots.iter().map(|s| s.k).collect();
        for n in 0..body.graphs.len() {
            for pt in s.instruments.points_of(r, x, def, n) {
                for k in pt.params().collect::<Vec<_>>() {
                    if taken.contains(&k) {
                        continue;
                    }
                    taken.push(k);
                    let spec = &def.params[k];
                    let mut v = s.instruments.turning(w.plugin, k).unwrap_or(now[k]);
                    // A reversed loop's end reads as the one it shows as, where it shows
                    // (low 1); it is still known by the value it moves, so Reverse does not
                    // make the reader lose its place (the sixth review's low 3).
                    let axis = match crate::device::sampler::mirror(def, &now, k) {
                        Some((shown, ends)) => {
                            v = crate::device::sampler::mirrored(spec, v, ends);
                            control::name(def, &def.params[shown])
                        }
                        None => control::name(def, spec),
                    };
                    let mut node = accesskit::Node::new(accesskit::Role::Slider);
                    node.set_label(if pt.name == axis {
                        axis.to_string()
                    } else {
                        format!("{} {axis}", pt.name)
                    });
                    node.set_value(control::text(spec, v));
                    numeric(&mut node, spec, v);
                    node.set_bounds(bounds(Rect::new(pt.at.0 - 8.0, pt.at.1 - 8.0, 16.0, 16.0)));
                    out.push(Mark {
                        key: format!("device-{}-point-{}", w.plugin, spec.id),
                        children: Vec::new(),
                        node,
                        fold: None,
                        pick: None,
                        press: None,
                        turn: Some((w.plugin, k)),
                        edge: None,
                    });
                }
            }
        }
        for slot in body.slots {
            let spec = &def.params[slot.k];
            let v = s
                .instruments
                .turning(w.plugin, slot.k)
                .unwrap_or(now[slot.k]);
            let mut node = match slot.look {
                Look::Toggle | Look::Dot => {
                    let mut n = accesskit::Node::new(accesskit::Role::Button);
                    n.set_toggled(if v >= 0.5 {
                        accesskit::Toggled::True
                    } else {
                        accesskit::Toggled::False
                    });
                    n
                }
                Look::Knob | Look::Segments | Look::Field => {
                    let mut n = accesskit::Node::new(accesskit::Role::Slider);
                    numeric(&mut n, spec, v);
                    n
                }
                Look::Menu => accesskit::Node::new(accesskit::Role::ComboBox),
            };
            // The Compressor names its Gain by what it drives (PLG-071).
            node.set_label(match (def.uid, slot.label) {
                ("compressor", Some(own)) => own,
                _ => control::name(def, spec),
            });
            // The Delay's Beats box says its note and feel (UI-329).
            node.set_value(match (def.uid, slot.look) {
                ("delay", Look::Menu) => {
                    let mut values = now.clone();
                    values[slot.k] = v;
                    control::menu_text(def, slot.k, &values)
                }
                _ => control::text(spec, v),
            });
            node.set_bounds(bounds(slot.hit(def)));
            let turns = matches!(slot.look, Look::Knob | Look::Segments | Look::Field);
            out.push(Mark {
                key: format!("device-{}-{}", w.plugin, spec.id),
                children: Vec::new(),
                node,
                fold: None,
                pick: None,
                press: None,
                turn: turns.then_some((w.plugin, slot.k)),
                edge: None,
            });
        }
    }
    out
}

/// A device's Slider reads in its own units (Hz, dB), from its least to its most, and steps
/// as an arrow key does from where it is (L2-ui-3).
fn numeric(node: &mut accesskit::Node, spec: &xirius_params::ParamSpec, v: f64) {
    use crate::device::control;
    let (lo, hi) = spec.range();
    let at = spec.to_plain(v);
    let up = spec.to_plain(control::step(spec, v, 1.0, control::Step::Small));
    let down = spec.to_plain(control::step(spec, v, -1.0, control::Step::Small));
    let step = (up - at).max(at - down);
    node.set_min_numeric_value(lo);
    node.set_max_numeric_value(hi);
    node.set_numeric_value(at);
    if step > 0.0 {
        node.set_numeric_value_step(step);
    }
}

/// A screen reader turns a device's control `k`: a step up or down as the arrow keys make
/// it, or to a value as if typed in (M-ui-2).
pub fn turn(s: &mut Studio, plugin: PluginId, k: usize, way: Turn) -> bool {
    s.device_a11y_turn(plugin, k, way)
}

/// A screen reader's turn of a velocity layer's edge (low 3): as the arrow keys move it, or to
/// the velocity it sets, in one step.
pub fn edge(s: &mut Studio, plugin: PluginId, j: usize, way: Turn) -> bool {
    s.layer_edge_turn(plugin, j, way)
}

/// What a screen reader does to a Slider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Turn {
    Up,
    Down,
    /// To this value in its own units (Hz, dB), as the Slider reads.
    To(f64),
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
    if s.sound_preparing() {
        return Vec::new();
    }
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
    // The start screen has no transport (UI-350).
    if s.home_shown() {
        return Vec::new();
    }
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

/// Persistent notifications expose both the message and their named actions.
pub fn toast_marks(s: &Studio, g: &Geometry) -> Vec<Mark> {
    use crate::toast::{self, ToastPart};
    if s.settings.is_some() || s.popup.is_some() {
        return vec![];
    }
    let (Some(t), Some(parts)) = (s.toast_shown(), toast::layout(s, g)) else {
        return vec![];
    };
    let mut out = vec![];
    let mut add = |key: &str, role, label: &str, rect, press| {
        let mut node = accesskit::Node::new(role);
        node.set_label(label);
        node.set_bounds(bounds(rect));
        out.push(Mark {
            key: key.into(),
            children: vec![],
            node,
            fold: None,
            pick: None,
            press,
            turn: None,
            edge: None,
        });
    };
    add(
        "toast-message",
        accesskit::Role::Status,
        &t.text,
        parts.text,
        None,
    );
    for (key, rect, button, part) in [
        ("toast-button", parts.button, &t.button, ToastPart::Button),
        ("toast-also", parts.also, &t.also, ToastPart::Also),
    ] {
        if let (Some(rect), Some((label, _))) = (rect, button) {
            add(
                key,
                accesskit::Role::Button,
                label,
                rect,
                Some(Hit::Toast(part)),
            );
        }
    }
    if let Some(rect) = parts.close {
        add(
            "toast-close",
            accesskit::Role::Button,
            "閉じる",
            rect,
            Some(Hit::Toast(ToastPart::Close)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Section;

    #[test]
    fn file_name_is_whole_in_accessibility_and_tooltip_when_visually_shortened() {
        use crate::browser::{BRow, BrowserHit, RailTab};
        let tmp = tempfile::tempdir().unwrap();
        let name = "長いファイル名_very_long_sample_name.wav";
        std::fs::write(tmp.path().join(name), []).unwrap();
        std::fs::write(tmp.path().join("z.wav"), []).unwrap();
        let mut s = Studio::sample();
        s.browser.press_rail(RailTab::Files);
        s.browser.add_place_fixture(tmp.path()).unwrap();
        crate::browser_files::settle_rows(&s.browser);
        let g = s.geometry(1200.0, 800.0);
        let l = s.browser.layout(g.panel);
        let i = l
            .rows
            .iter()
            .position(|p| matches!(&p.row, BRow::File { name: n, .. } if n == name))
            .unwrap();
        let hit = crate::studio::Hit::Browser(BrowserHit::File(i));
        assert_eq!(crate::hover_tip::text(&s, hit).as_deref(), Some(name));
        let marks = file_marks(&s, &g);
        let mark = marks.iter().find(|m| m.node.label() == Some(name)).unwrap();
        assert_eq!(mark.node.role(), accesskit::Role::ListItem);
        assert_eq!(mark.press, Some(hit));
        let keys: Vec<_> = marks.iter().map(|m| m.key.clone()).collect();
        s.browser
            .select_file(&tmp.path().join(name).to_string_lossy());
        // Inserting the audition row must not change the following file's identity.
        assert_eq!(
            file_marks(&s, &g)
                .iter()
                .map(|m| m.key.clone())
                .collect::<Vec<_>>(),
            keys
        );
        s.browser.open = false;
        assert!(file_marks(&s, &g).is_empty());
    }

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
        assert!(s.context_choose(&g, "クリップ"));
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
        // The drawer also has accessible controls; this test counts only take badges.
        let take_marks = |s: &Studio| {
            marks(s, &g)
                .into_iter()
                .filter(|m| m.fold.is_some())
                .collect::<Vec<_>>()
        };
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
        assert!(take_marks(&s).is_empty());
        record(&mut s, vocal, 3);
        let marks_now = take_marks(&s);
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
        assert_eq!(take_marks(&s)[0].node.is_expanded(), Some(true));
        assert!(fold(&mut s, vocal));
        assert_eq!(take_marks(&s)[0].node.is_expanded(), Some(false));
        // A pop-over takes the presses: the counts under it are not offered.
        s.open_popup(
            crate::popup::PopupKind::Song,
            Rect::new(200.0, 200.0, 120.0, 24.0),
        );
        assert!(take_marks(&s).is_empty());
        s.popup = None;
        assert_eq!(take_marks(&s).len(), 1);
        // Scrolled away, it is not offered either.
        s.scroll.0 = 0.0;
        assert!(take_marks(&s).is_empty());
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
