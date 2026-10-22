//! A channel's name dragged in the mixer. Dropped on the middle of another channel, that
//! channel takes its settings (MIX-007, `channel.settings.paste --from`). Dropped on the edge
//! of a channel of its own kind (tracks among tracks, buses among buses), it moves there,
//! before or after it (UI-170, `track.move`): the mixer's order is the order of the tracks, so
//! the song keeps it. Next to a track in a folder, it goes into that folder.

use serde_json::json;
use xirius_core::id::{Node, TrackId};
use xirius_core::model::{Parent, TrackKind};
use xirius_design::{FontSize, Radius, Weight};

use crate::draw::{Align, Pen};
use crate::mixer::{MixerHit, STRIP_GAP};
use crate::studio::{Geometry, Hit, Rect, Studio};

/// How far the pointer goes before a press on a name is a drag.
const SLOP: f32 = 4.0;

/// The part of a strip's width, on each side, where a drop moves the channel next to it.
const EDGE: f32 = 0.25;

/// What a release does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    /// The channel goes before (or after) `anchor`.
    Move { anchor: TrackId, after: bool },
    /// The channel takes the dragged one's settings.
    Paste(TrackId),
}

/// The channels a channel moves among; none for the master.
fn group(kind: TrackKind) -> Option<u8> {
    match kind {
        TrackKind::Audio | TrackKind::Instrument => Some(0),
        TrackKind::Bus => Some(1),
        TrackKind::Master => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub from: TrackId,
    start: (f32, f32),
    pub at: (f32, f32),
    pub moved: bool,
}

impl Studio {
    /// A press on a channel's name starts the drag; the press goes on selecting the channel.
    pub fn name_drag_down(&mut self, g: &Geometry, x: f32, y: f32) {
        self.mixer.name_drag = None;
        if self.popup.is_some() {
            return;
        }
        let Hit::Mixer(MixerHit::Strip(t)) = self.hit(g, x, y) else {
            return;
        };
        let on_name = self
            .mixer_strips(g)
            .into_iter()
            .find(|s| s.track == t)
            .is_some_and(|s| s.name.contains(x, y));
        if on_name {
            self.mixer.name_drag = Some(Drag {
                from: t,
                start: (x, y),
                at: (x, y),
                moved: false,
            });
        }
    }

    pub fn name_drag_move(&mut self, x: f32, y: f32) -> bool {
        let Some(d) = &mut self.mixer.name_drag else {
            return false;
        };
        d.at = (x, y);
        d.moved |= (x - d.start.0).hypot(y - d.start.1) > SLOP;
        d.moved
    }

    /// The release: on the edge of a channel, the channel moves next to it; on the middle of
    /// another, that one takes the settings. True when it was a drag (so the press is not a
    /// click as well).
    pub fn name_drag_up(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(d) = self.mixer.name_drag.take() else {
            return false;
        };
        if !d.moved {
            return false;
        }
        match self.name_drop(g, d.from, x, y) {
            Some(Drop::Move { anchor, after }) => self.strip_move(g, d.from, anchor, after),
            Some(Drop::Paste(to)) => {
                self.arrange_run(
                    "channel.settings.paste",
                    json!({ "ids": [to.to_string()], "from": d.from.to_string() }),
                );
                self.rebuild(g);
            }
            None => {}
        }
        true
    }

    /// What a release of `from` at a point does.
    pub fn name_drop(&self, g: &Geometry, from: TrackId, x: f32, y: f32) -> Option<Drop> {
        let p = self.project();
        let view = self.mixer_view(g);
        let kind = |t: TrackId| p.tracks.get(&t).map(|t| t.kind);
        let mine = kind(from).and_then(group);
        // The gap between two strips counts as the edge of the one on its left.
        let near = self.mixer_strips(g).into_iter().find(|s| {
            let r = s.rect;
            !s.track.is_master()
                && view.contains(x, y)
                && x >= r.x - STRIP_GAP
                && x < r.right() + STRIP_GAP
                && y >= r.y
                && y < r.bottom()
        });
        if let Some(s) = near.filter(|s| mine.is_some() && kind(s.track).and_then(group) == mine) {
            let r = s.rect;
            let edge = r.w * EDGE;
            let after = if x < r.x + edge {
                Some(false)
            } else if x >= r.right() - edge {
                Some(true)
            } else {
                None
            };
            if let Some(after) = after {
                if s.track == from {
                    return None;
                }
                // A place that leaves the strips as they are is none.
                let order = crate::mixer::strip_order(&p);
                let i = order.iter().position(|t| *t == s.track)?;
                let next = if after {
                    order.get(i + 1)
                } else {
                    i.checked_sub(1).and_then(|j| order.get(j))
                };
                if next == Some(&from)
                    || p.find_node(Node::Track(from)).is_none()
                    || p.find_node(Node::Track(s.track)).is_none()
                {
                    return None;
                }
                return Some(Drop::Move {
                    anchor: s.track,
                    after,
                });
            }
        }
        self.strip_under(g, x, y)
            .filter(|to| *to != from)
            .map(Drop::Paste)
    }

    /// Moves `from` before or after `anchor`, into the folder `anchor` is in, in one step.
    fn strip_move(&mut self, g: &Geometry, from: TrackId, anchor: TrackId, after: bool) {
        let p = self.project();
        let Some((parent, i)) = p.find_node(Node::Track(anchor)) else {
            return;
        };
        // The index counts the list after the moving track is taken out.
        let mut index = i + usize::from(after);
        if let Some((there, j)) = p.find_node(Node::Track(from))
            && there == parent
            && j < index
        {
            index -= 1;
        }
        let into = match parent {
            Parent::Root => "root".to_string(),
            Parent::Folder(f) => f.to_string(),
        };
        self.arrange_run(
            "track.move",
            json!({ "ids": [from.to_string()], "into": into, "index": index }),
        );
        self.rebuild(g);
    }

    /// The channel whose strip holds a point.
    pub fn strip_under(&self, g: &Geometry, x: f32, y: f32) -> Option<TrackId> {
        self.mixer_hit(g, x, y).map(MixerHit::track)
    }
}

/// Where the release goes: a line in the gap the channel moves to, or the strip that takes
/// the settings lit; and the name following the pointer.
pub fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let Some(d) = s.mixer.name_drag.filter(|d| d.moved) else {
        return;
    };
    let c = pen.c;
    let strip = |t: TrackId| s.mixer_strips(g).into_iter().find(|x| x.track == t);
    match s.name_drop(g, d.from, d.at.0, d.at.1) {
        Some(Drop::Move { anchor, after }) => {
            if let Some(strip) = strip(anchor) {
                let r = strip.rect;
                let x = if after {
                    r.right() + STRIP_GAP / 2.0
                } else {
                    r.x - STRIP_GAP / 2.0
                };
                let view = s.mixer_view(g);
                pen.mask(view, |pen| {
                    pen.round(Rect::new(x - 1.5, r.y, 3.0, r.h), c.fill, Radius::Pill);
                });
            }
        }
        Some(Drop::Paste(to)) => {
            if let Some(strip) = strip(to) {
                pen.fill(strip.rect, c.fill_hover.opacity(0.3));
            }
        }
        None => {}
    }
    let name = s
        .project()
        .tracks
        .get(&d.from)
        .map(|t| t.name.clone())
        .unwrap_or_default();
    let r = Rect::new(
        (d.at.0 + 12.0).min(g.size.0 - 132.0),
        (d.at.1 + 12.0).min(g.size.1 - 28.0),
        120.0,
        24.0,
    );
    pen.round(r, c.surface_3, Radius::S);
    pen.text(
        &name,
        FontSize::Caption,
        Weight::Medium,
        c.ink_strong,
        r,
        Align::Center,
    );
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use xirius_core::clipboard::Clipboard;

    use super::*;
    use crate::studio::DrawerTab;
    use crate::tools::Mods;

    const CTRL: Mods = Mods {
        ctrl: true,
        ..Mods::NONE
    };

    fn mix() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        let g = s.geometry(1600.0, 1000.0);
        (s, g)
    }

    fn track(s: &Studio, name: &str) -> TrackId {
        crate::arrange::probe::track(s, name)
    }

    fn name_of(s: &Studio, g: &Geometry, t: TrackId) -> (f32, f32) {
        let r = s
            .mixer_strips(g)
            .into_iter()
            .find(|x| x.track == t)
            .expect("a strip")
            .name;
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// The left press on a channel's name, as the window does it.
    fn press_name(s: &mut Studio, g: &Geometry, t: TrackId) {
        let (x, y) = name_of(s, g, t);
        s.name_drag_down(g, x, y);
        let hit = s.mixer_hit(g, x, y).expect("the name is in a strip");
        assert!(s.mixer_press(g, hit));
    }

    fn add_effect(s: &mut Studio, t: TrackId, name: &str) {
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": name, "kind": "effect", "format": "builtin", "uid": name }),
        ));
    }

    fn inserts(s: &Studio, t: TrackId) -> Vec<String> {
        s.project().tracks[&t]
            .inserts
            .iter()
            .map(|p| p.name.clone())
            .collect()
    }

    fn steps(s: &Studio) -> usize {
        s.song.steps()
    }

    #[test]
    fn ctrl_c_on_a_selected_channel_copies_its_settings_and_ctrl_v_pastes_in_one_step() {
        let (mut s, g) = mix();
        let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
        add_effect(&mut s, vocal, "Compressor");
        press_name(&mut s, &g, vocal);
        assert_eq!(s.input_context(&g), "mixer");
        assert!(s.mapped_key(&g, "c", None, CTRL));
        assert!(matches!(
            s.song.session().state.clipboard,
            Some(Clipboard::Channel(_))
        ));
        press_name(&mut s, &g, guitar);
        let before = steps(&s);
        assert!(s.mapped_key(&g, "v", None, CTRL));
        assert_eq!(inserts(&s, guitar), ["Compressor"]);
        assert_eq!(steps(&s), before + 1);
        assert!(s.undo(&g));
        assert!(inserts(&s, guitar).is_empty());
    }

    #[test]
    fn the_master_is_copied_and_pasted_from_the_keys_too() {
        let (mut s, g) = mix();
        let keys = track(&s, "Keys");
        press_name(&mut s, &g, TrackId::MASTER);
        assert!(s.mapped_key(&g, "c", None, CTRL));
        assert!(matches!(
            s.song.session().state.clipboard,
            Some(Clipboard::Channel(ref c)) if c.source == TrackId::MASTER
        ));
        press_name(&mut s, &g, keys);
        assert!(s.mapped_key(&g, "v", None, CTRL));
        assert!(s.notice_text().is_some_and(|t| t.contains("貼り付け")));
    }

    #[test]
    fn a_plugin_picked_with_the_right_button_still_copies_as_a_plugin() {
        let (mut s, g) = mix();
        let vocal = track(&s, "Vocal");
        add_effect(&mut s, vocal, "Compressor");
        let pid = s.project().tracks[&vocal].inserts[0].id;
        s.input_map.context_clipboard = Some(json!({ "plugin": pid.to_string() }));
        assert!(s.mapped_key(&g, "c", None, CTRL));
        assert!(matches!(
            s.song.session().state.clipboard,
            Some(Clipboard::Plugins(_))
        ));
    }

    #[test]
    fn dragging_a_name_onto_another_channel_pastes_its_settings_in_one_step() {
        let (mut s, g) = mix();
        let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
        add_effect(&mut s, vocal, "Compressor");
        let before = steps(&s);
        press_name(&mut s, &g, vocal);
        let (gx, gy) = name_of(&s, &g, guitar);
        assert!(s.name_drag_move(gx, gy), "the drag has begun");
        assert!(s.name_drag_up(&g, gx, gy));
        assert_eq!(inserts(&s, guitar), ["Compressor"]);
        assert_eq!(steps(&s), before + 1);
        assert!(s.mixer.name_drag.is_none());
        assert!(s.undo(&g));
        assert!(inserts(&s, guitar).is_empty());
    }

    #[test]
    fn a_press_without_a_move_and_a_drop_on_itself_do_nothing() {
        let (mut s, g) = mix();
        let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
        add_effect(&mut s, vocal, "Compressor");
        let before = steps(&s);
        press_name(&mut s, &g, vocal);
        let (x, y) = name_of(&s, &g, vocal);
        assert!(!s.name_drag_move(x + 1.0, y));
        assert!(!s.name_drag_up(&g, x + 1.0, y));
        press_name(&mut s, &g, vocal);
        assert!(s.name_drag_move(x + 30.0, y + 60.0));
        assert!(s.name_drag_up(&g, x, y));
        assert_eq!(steps(&s), before);
        assert!(inserts(&s, guitar).is_empty());
    }

    /// The middle of the left (or right) edge of a strip.
    fn edge_of(s: &Studio, g: &Geometry, t: TrackId, after: bool) -> (f32, f32) {
        let r = s
            .mixer_strips(g)
            .into_iter()
            .find(|x| x.track == t)
            .expect("a strip")
            .rect;
        let x = if after { r.right() - 3.0 } else { r.x + 3.0 };
        (x, r.y + r.h / 2.0)
    }

    fn order(s: &Studio) -> Vec<String> {
        crate::mixer::strip_order(&s.project())
            .into_iter()
            .map(|t| s.project().tracks[&t].name.clone())
            .collect()
    }

    fn drag_to(s: &mut Studio, g: &Geometry, t: TrackId, (x, y): (f32, f32)) {
        press_name(s, g, t);
        assert!(s.name_drag_move(x, y), "the drag has begun");
        assert!(s.name_drag_up(g, x, y));
    }

    #[test]
    fn a_name_dropped_on_the_edge_of_a_strip_moves_the_channel_there_in_one_step() {
        let (mut s, g) = mix();
        let names = order(&s);
        let first = track(&s, &names[0]);
        let third = track(&s, &names[2]);
        let before = steps(&s);
        // The first after the third.
        let at = edge_of(&s, &g, third, true);
        assert_eq!(
            s.name_drop(&g, first, at.0, at.1),
            Some(Drop::Move {
                anchor: third,
                after: true
            })
        );
        drag_to(&mut s, &g, first, at);
        let mut want = names.clone();
        let moved = want.remove(0);
        want.insert(2, moved);
        assert_eq!(order(&s), want);
        assert_eq!(steps(&s), before + 1);
        // And back before the old second, which is first now.
        let second = track(&s, &names[1]);
        let at = edge_of(&s, &g, second, false);
        drag_to(&mut s, &g, first, at);
        assert_eq!(order(&s), names);
        assert!(s.undo(&g));
        assert_eq!(order(&s), want);
        // Nothing pasted on the way.
        assert!(s.project().tracks[&third].inserts.is_empty());
    }

    #[test]
    fn the_order_the_mixer_takes_is_kept_when_the_song_is_opened_again() {
        let (mut s, g) = mix();
        let names = order(&s);
        let first = track(&s, &names[0]);
        let last_track = s
            .project()
            .tracks
            .values()
            .filter(|t| matches!(t.kind, TrackKind::Audio | TrackKind::Instrument))
            .count()
            - 1;
        let anchor = track(&s, &names[last_track]);
        let at = edge_of(&s, &g, anchor, true);
        drag_to(&mut s, &g, first, at);
        let moved = order(&s);
        assert_ne!(moved, names);
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("order");
        s.command(|song| song.save_as(&dir));
        assert!(s.error.is_none(), "{:?}", s.error);
        s.new_song();
        s.finish_file_request(crate::studio::FileRequest::Open, &dir);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(order(&s), moved);
    }

    #[test]
    fn a_track_does_not_move_among_the_buses_and_a_place_it_already_has_is_none() {
        let (s, g) = mix();
        let names = order(&s);
        let first = track(&s, &names[0]);
        let second = track(&s, &names[1]);
        let p = s.project();
        let bus = p
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Bus)
            .map(|t| t.id);
        // Right of itself, or left of the one after it: where it is.
        let at = edge_of(&s, &g, second, false);
        assert_eq!(s.name_drop(&g, first, at.0, at.1), None);
        let at = edge_of(&s, &g, first, true);
        assert_eq!(s.name_drop(&g, first, at.0, at.1), None);
        // The edge of a bus is the bus: it takes the settings.
        if let Some(bus) = bus
            && s.mixer_strips(&g).iter().any(|x| x.track == bus)
        {
            let at = edge_of(&s, &g, bus, false);
            assert_eq!(s.name_drop(&g, first, at.0, at.1), Some(Drop::Paste(bus)));
        }
        // The master does not move.
        let at = edge_of(&s, &g, first, false);
        assert_eq!(
            s.name_drop(&g, TrackId::MASTER, at.0, at.1),
            Some(Drop::Paste(first))
        );
    }

    #[test]
    fn a_press_off_the_name_does_not_begin_a_drag() {
        let (mut s, g) = mix();
        let vocal = track(&s, "Vocal");
        let strip = s
            .mixer_strips(&g)
            .into_iter()
            .find(|x| x.track == vocal)
            .unwrap();
        s.name_drag_down(&g, strip.fader.x + 2.0, strip.fader.y + 2.0);
        assert!(s.mixer.name_drag.is_none());
    }
}
