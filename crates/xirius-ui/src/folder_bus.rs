//! A folder's output bus (EDT-073, the mock's folder head): a button at the right of the
//! folder's row, with the bus icon and the bus's name when one is connected, and its menu of
//! つながない, the buses and 新しいバス. A bus row runs `folder.bus.set`; 新しいバス runs
//! `folder.bus.add`, which makes the bus and connects it in one step (one undo).

use serde_json::json;
use xirius_core::id::{FolderId, TrackId};
use xirius_core::model::{Project, TrackKind};
use xirius_design::{FontSize, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::popup::{Action, Choice, PopupKind};
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tree::Row;

/// The button, as in the mock: 20 high, 6 at each side, at most 84 wide.
pub const H: f32 = 20.0;
pub const PAD_X: f32 = 6.0;
pub const ICON: f32 = 14.0;
pub const GAP: f32 = 3.0;
pub const MAX_W: f32 = 84.0;

/// What the menu row of a new bus sends.
pub const NEW: &str = "new";
/// What the menu row of つながない sends.
pub const NONE: &str = "none";

/// The bus a folder plays to, if it still exists.
pub fn bus_of(p: &Project, f: FolderId) -> Option<(TrackId, &str)> {
    let id = p.folders.get(&f)?.bus?;
    p.tracks.get(&id).map(|t| (id, t.name.as_str()))
}

/// Where the button sits in the row (row coordinates): at the right of the head.
pub fn rect(row: &Row, column_w: f32, name: Option<&str>) -> Rect {
    if row.bus.is_some() {
        return Rect::new(
            column_w.max(row.head_w) - Space::X2.px() - crate::studio::RowParts::METER_W - 24.0,
            6.0,
            20.0,
            24.0,
        );
    }
    let w = match name {
        Some(n) => {
            (PAD_X * 2.0 + ICON + GAP + crate::browser::text_width(n, FontSize::Caption)).min(MAX_W)
        }
        None => PAD_X * 2.0 + ICON,
    };
    let right = column_w.max(row.head_w) - Space::X2.px();
    Rect::new(right - w, (row.h - H) / 2.0, w, H)
}

/// The menu: つながない, then the buses, then 新しいバス after a gap.
pub fn choices(p: &Project) -> Vec<Choice> {
    let mut out = vec![Choice {
        to: NONE.into(),
        label: "つながない".into(),
    }];
    out.extend(
        p.tracks
            .values()
            .filter(|t| t.kind == TrackKind::Bus)
            .map(|t| Choice {
                to: t.id.to_string(),
                label: t.name.clone(),
            }),
    );
    out
}

/// The menu as a list menu (UI-320): つながない and the buses, then
/// 新しいバス after a gap.
pub fn pick(choices: Vec<Choice>, current: &str) -> crate::pick_list::PickList {
    use crate::pick_list::{PickGroup, PickList, PickRow};
    let mut list = PickList::of(Vec::new());
    list.groups = vec![
        PickGroup::new(
            "",
            choices
                .into_iter()
                .map(|c| {
                    let mut row = PickRow::new(c.label)
                        .on(c.to == current)
                        .act(Action::FolderBus(c.to.clone()));
                    if c.to != NONE {
                        row = row.icon("bus");
                    }
                    row
                })
                .collect(),
        ),
        PickGroup::new(
            "",
            vec![
                PickRow::new("新しいバス")
                    .icon("plus")
                    .act(Action::FolderBus(NEW.into())),
            ],
        ),
    ];
    list.named("見つかりません")
}

/// The button on a folder's row. `top` is the row's top on screen.
pub fn paint(
    pen: &mut Pen,
    studio: &Studio,
    p: &Project,
    g: &Geometry,
    row: &Row,
    top: f32,
    f: FolderId,
) {
    let c = pen.c;
    let bus = bus_of(p, f);
    let r = rect(row, g.column.w, bus.map(|(_, n)| n));
    let r = Rect::new(g.column.x + r.x, top + r.y, r.w, r.h);
    if row.bus.is_some() {
        if studio.hover == Hit::FolderBus(f) {
            pen.round(r, c.hover, Radius::Clip);
        }
        pen.icon(
            "effects",
            Rect::new(r.x + 2.0, r.y + 4.0, 16.0, 16.0),
            c.ink,
        );
        return;
    }
    let open = studio
        .popup
        .as_ref()
        .is_some_and(|p| p.kind == PopupKind::FolderBus(f));
    if open || studio.hover == Hit::FolderBus(f) {
        pen.round(r, c.surface_3, Radius::Clip);
    }
    let icon = Rect::new(r.x + PAD_X, r.y + (H - ICON) / 2.0, ICON, ICON);
    pen.icon("bus", icon, c.ink);
    if let Some((_, name)) = bus {
        let x = icon.right() + GAP;
        let text_r = Rect::new(x, r.y, r.right() - PAD_X - x, r.h);
        pen.mask(text_r, |pen| {
            pen.text(
                name,
                FontSize::Caption,
                Weight::Medium,
                pen.c.ink,
                text_r,
                Align::Left,
            );
        });
    }
}

impl Studio {
    /// The button's rect on the folder's row (row coordinates).
    pub(crate) fn folder_bus_rect(&self, row: &Row, column_w: f32, f: FolderId) -> Rect {
        let p = self.project();
        rect(row, column_w, bus_of(&p, f).map(|(_, n)| n))
    }

    /// The middle of a folder's bus button on screen.
    pub(crate) fn folder_bus_center(&self, g: &Geometry, f: FolderId) -> Option<(f32, f32)> {
        let row = self.tree().row_of(crate::tree::RowKind::Folder(f))?;
        let r = self.folder_bus_rect(row, g.column.w, f);
        let top = self.row_y(g, row);
        Some((g.column.x + r.x + r.w / 2.0, top + r.y + r.h / 2.0))
    }

    /// The press on the button: its menu under it.
    pub(crate) fn open_folder_bus(&mut self, g: &Geometry, f: FolderId) -> bool {
        if let Some(bus) = self.project().row_bus(f) {
            self.drawer_open = true;
            self.tab = crate::studio::DrawerTab::Mixer;
            self.select_row(Some(crate::tree::RowKind::Folder(f)));
            self.mixer.heading = Some(bus);
            self.mixer.scroll = self
                .strip_offset(g, bus)
                .unwrap_or(0.0)
                .min(self.mixer_max_scroll(g));
            return true;
        }
        let Some(row) = self.tree().row_of(crate::tree::RowKind::Folder(f)) else {
            return false;
        };
        let r = self.folder_bus_rect(row, g.column.w, f);
        let anchor = Rect::new(g.column.x + r.x, self.row_y(g, row) + r.y, r.w, r.h);
        self.open_popup(PopupKind::FolderBus(f), anchor);
        true
    }

    /// The menu's rows and which one is lit.
    pub(crate) fn folder_bus_view(&self, f: FolderId) -> (Vec<Choice>, String) {
        let p = self.project();
        let current = bus_of(&p, f).map_or(NONE.to_string(), |(id, _)| id.to_string());
        (choices(&p), current)
    }

    /// A row of the menu: connects the folder, or makes a bus and connects it.
    pub(crate) fn folder_bus_action(&mut self, g: &Geometry, to: String) -> bool {
        let Some(PopupKind::FolderBus(f)) = self.popup.as_ref().map(|p| p.kind) else {
            return false;
        };
        self.popup = None;
        if to == NEW {
            // Named as the folder and made right under it, as a new folder's bus is.
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.run_command("folder.bus.add", json!({ "id": f.to_string() }));
        } else {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.run_command("folder.bus.set", json!({ "id": f.to_string(), "bus": to }));
        }
        self.rebuild(g);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::RowKind;
    use xirius_core::id::Node;

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    fn rhythm(s: &Studio) -> FolderId {
        let p = s.project();
        *p.folders
            .iter()
            .find(|(_, f)| f.name == "Rhythm")
            .unwrap()
            .0
    }

    fn add_bus(s: &mut Studio, name: &str) -> TrackId {
        s.song
            .run("track.add.bus", json!({ "name": name }))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    /// Presses the menu row that says `text`.
    fn choose(s: &mut Studio, g: &Geometry, text: &str) {
        let l = s.popup.as_ref().unwrap().layout(g.size);
        let i = l.cells.iter().position(|c| c.text == text).unwrap();
        let c = &l.cells[i];
        let hit = s.hit(g, c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0);
        assert_eq!(hit, Hit::Item(i));
        assert!(s.press(g, hit));
    }

    #[test]
    fn the_button_sits_on_the_folder_row_and_opens_the_menu() {
        let (mut s, g) = studio();
        let f = rhythm(&s);
        add_bus(&mut s, "Groove");
        let (x, y) = s.center_of(&g, Hit::FolderBus(f)).unwrap();
        assert_eq!(s.hit(&g, x, y), Hit::FolderBus(f));
        assert!(s.press(&g, Hit::FolderBus(f)));
        let p = s.popup.as_ref().unwrap();
        assert_eq!(p.kind, PopupKind::FolderBus(f));
        let l = p.layout(g.size);
        let texts: Vec<_> = l.cells.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["", "つながない", "Groove", "新しいバス"]);
        // Not connected: つながない is lit, and the bus rows carry the bus icon.
        // The first cell is the search box (UI-320): the rows follow it.
        assert!(l.cells[1].on && !l.cells[2].on);
        assert_eq!(l.cells[2].icon, Some("bus"));
        assert_eq!(l.cells[3].icon, Some("plus"));
        // The menu opens under the button.
        assert!(l.rect.y >= y);
        // A second press shuts it.
        s.press(&g, Hit::FolderBus(f));
        assert!(s.popup.is_none());
    }

    #[test]
    fn a_bus_connects_and_the_button_names_it() {
        let (mut s, g) = studio();
        let f = rhythm(&s);
        let groove = add_bus(&mut s, "Groove");
        s.after_edit();
        let row = *s.tree().row_of(RowKind::Folder(f)).unwrap();
        let bare = s.folder_bus_rect(&row, row.head_w, f);
        s.press(&g, Hit::FolderBus(f));
        choose(&mut s, &g, "Groove");
        assert!(s.popup.is_none());
        assert_eq!(s.project().folders[&f].bus, Some(groove));
        // Wider with the name, never past 84, still at the head's right.
        let named = s.folder_bus_rect(&row, row.head_w, f);
        assert!(named.w > bare.w && named.w <= MAX_W);
        assert_eq!(named.right(), bare.right());
        // The menu lights it; つながない takes it off.
        s.press(&g, Hit::FolderBus(f));
        let l = s.popup.as_ref().unwrap().layout(g.size);
        assert!(l.cells.iter().any(|c| c.text == "Groove" && c.on));
        choose(&mut s, &g, "つながない");
        assert_eq!(s.project().folders[&f].bus, None);
    }

    #[test]
    fn a_new_bus_is_made_and_connected_and_one_undo_takes_both_away() {
        let (mut s, g) = studio();
        // Undo here must land exactly on the state the test kept; the
        // instrument-input arm that would follow the selection is not
        // what this test checks (REC-024).
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
        );
        let f = rhythm(&s);
        let before = s.project().content_hash();
        let tracks = s.project().tracks.len();
        s.press(&g, Hit::FolderBus(f));
        choose(&mut s, &g, "新しいバス");
        assert!(s.error.is_none());
        {
            let p = s.project();
            assert_eq!(p.tracks.len(), tracks + 1);
            let bus = p.folders[&f].bus.expect("connected");
            assert_eq!(p.tracks[&bus].kind, TrackKind::Bus);
            // The channel belongs to the folder row.
            assert_eq!(p.tracks[&bus].name, "Rhythm");
            assert_eq!(
                p.find_node(Node::Track(bus)),
                Some((xirius_core::model::Parent::Folder(f), 0))
            );
        }
        // The row offers its insert entry instead of the old bus-link menu.
        let row = *s.tree().row_of(RowKind::Folder(f)).unwrap();
        assert_eq!(row.h, 96.0);
        assert!(row.bus.is_some());
        // One undo takes the bus and the connection away together.
        s.undo(&g);
        assert_eq!(s.project().content_hash(), before);
        assert_eq!(s.project().folders[&f].bus, None);
    }

    fn owned(s: &mut Studio, name: &str) -> (FolderId, TrackId) {
        let out = s.song.run("folder.add", json!({"name":name})).unwrap();
        s.after_edit();
        (
            out["id"].as_str().unwrap().parse().unwrap(),
            out["bus"].as_str().unwrap().parse().unwrap(),
        )
    }

    #[test]
    fn folded_folder_keeps_channel_controls_and_opens_its_mixer_inserts() {
        let (mut s, _) = studio();
        let (f, bus) = owned(&mut s, "Group");
        let g = s.geometry(1440.0, 1000.0);
        s.select_row(Some(RowKind::Folder(f)));
        let children: Vec<_> = s
            .project()
            .tracks
            .iter()
            .filter(|(id, _)| **id != bus)
            .map(|(id, t)| (*id, t.mute))
            .collect();
        for collapsed in [false, true] {
            if collapsed {
                s.press(&g, Hit::Chevron(f));
            }
            let row = *s.tree().row_of(RowKind::Folder(f)).unwrap();
            assert_eq!(row.h, 96.0);
            assert_eq!(row.channel(), Some(bus));
            assert_eq!(
                s.tree()
                    .rows
                    .iter()
                    .filter(|r| r.channel() == Some(bus))
                    .count(),
                1
            );
            s.scroll.1 = row.y;
            for hit in [
                Hit::Mute(bus),
                Hit::Solo(bus),
                Hit::HeadFader(bus),
                Hit::Output(bus),
                Hit::FolderBus(f),
            ] {
                let (x, y) = s.center_of(&g, hit).unwrap();
                assert_eq!(s.hit(&g, x, y), hit);
            }
            assert!(g.parts(&row).arm.is_none());
            assert!(g.parts(&row).ear.is_none());
            assert!(g.parts(&row).meter.is_some());
        }
        s.press(&g, Hit::Mute(bus));
        assert!(s.project().tracks[&bus].mute);
        s.press(&g, Hit::Solo(bus));
        assert!(s.project().tracks[&bus].solo);
        s.head_fader.focus = Some(bus);
        assert!(s.head_fader_key("left", crate::tools::Mods::default()));
        let db = s.project().tracks[&bus].volume_db;
        assert!(db < 0.0);
        assert_eq!(s.head_fader_value(bus), Some(db));
        assert_eq!(s.mixer_value(crate::mixer::MixerHit::Fader(bus)), Some(db));
        for (id, mute) in children {
            assert_eq!(s.project().tracks[&id].mute, mute);
        }
        s.press(&g, Hit::FolderBus(f));
        assert!(s.mixer_shown());
        assert_eq!(s.mixer.heading, Some(bus));
        assert_eq!(s.selected_row, Some(RowKind::Folder(f)));
        assert!(s.is_picked(RowKind::Track(bus)));
    }

    #[test]
    fn dropping_below_a_folder_head_keeps_its_channel_first() {
        let (mut s, g) = studio();
        let (f, b) = owned(&mut s, "Group");
        let t = add_bus(&mut s, "Outside");
        let row = *s.tree().row_of(RowKind::Folder(f)).unwrap();
        s.scroll.1 = row.y;
        let (parent, index, mark) = s
            .drop_at(&g, g.column.y + row.h * 0.85, &[RowKind::Track(t)])
            .unwrap();
        assert_eq!(parent, xirius_core::model::Parent::Folder(f));
        assert_eq!(index, 1);
        assert!(matches!(mark, crate::arrange::DropMark::Line { .. }));
        s.song
            .run("track.move", json!({"ids":[t], "into":f, "index":index}))
            .unwrap();
        s.after_edit();
        assert_eq!(
            s.project().folders[&f].children,
            [Node::Track(b), Node::Track(t)]
        );
        let tree = s.tree();
        let visible = tree
            .rows
            .iter()
            .position(|r| r.kind == RowKind::Folder(f))
            .unwrap();
        assert_eq!(tree.rows[visible + 1].kind, RowKind::Track(t));
    }

    #[test]
    fn nested_folder_output_choices_exclude_cycles_and_bus_order_ignores_folding() {
        let (mut s, g) = studio();
        let (outer, a) = owned(&mut s, "Outer");
        let (inner, b) = owned(&mut s, "Inner");
        s.song
            .run(
                "track.move",
                json!({"ids":[inner.to_string()], "into":outer.to_string()}),
            )
            .unwrap();
        s.after_edit();
        let choices = crate::mixer::output_choices(&s.project(), a);
        assert!(
            !choices
                .iter()
                .any(|c| c.to == b.to_string() || c.to == a.to_string())
        );
        assert_eq!(
            crate::draw::output_label(&s.project(), &s.project().tracks[&b]),
            "Outer"
        );
        let before = crate::mixer::strip_order(&s.project());
        assert!(before.iter().position(|t| *t == a) < before.iter().position(|t| *t == b));
        s.press(&g, Hit::Chevron(outer));
        assert_eq!(crate::mixer::strip_order(&s.project()), before);
        s.song
            .run(
                "track.output.set",
                json!({"ids":[b.to_string()], "to":"master"}),
            )
            .unwrap();
        let choices = crate::mixer::output_choices(&s.project(), a);
        assert!(choices.iter().any(|c| c.to == b.to_string()));
    }

    #[test]
    fn bus_automation_lanes_are_independent_of_child_folding() {
        let (mut s, _) = studio();
        let (f, b) = owned(&mut s, "Group");
        let lanes = std::collections::HashMap::from([(b, 2)]);
        let p = s.project();
        for collapsed in [
            std::collections::HashSet::new(),
            std::collections::HashSet::from([f]),
        ] {
            let tree = crate::tree::Tree::build_shaped(
                &p,
                &collapsed,
                &std::collections::HashSet::new(),
                64.0,
                &lanes,
            );
            let row = tree.row_of(RowKind::Folder(f)).unwrap();
            assert_eq!(row.lanes, 2);
            assert_eq!(row.full_h(), 208.0);
            assert_eq!(
                tree.row_of(RowKind::Track(b)).unwrap().kind,
                RowKind::Folder(f)
            );
        }
    }
}
