//! KEY-012: the screen's side of the rest of the Studio Pro key table — the commands that take
//! what is selected or shown (the selected clips, the selected track, the view in front), and the
//! keys that open a command's form where the table's own needs a target (find a track, recall a
//! marker). A typed command, the palette and an AI's run come here by the same names.

use serde_json::{Map, Value, json};
use xirius_core::id::{ClipId, PluginId, TrackId};
use xirius_core::model::TrackKind;
use xirius_core::time::Tick;

use crate::browser::{RailTab, View};
use crate::studio::{DrawerTab, Geometry, Studio};
use crate::tree::RowKind;

/// What the window itself does for a command: it has the window (KEY-012).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowAsk {
    Fullscreen,
    Quit,
}

/// The parts of the window the focus goes round (Tab): the arrangement, the dock, the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Arrange,
    Dock,
    Browser,
}

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)?.as_str().filter(|s| !s.trim().is_empty())
}

impl Studio {
    /// The commands of this module. `None` for a name that is not one of them (or has the
    /// arguments of the table's own), `Some(false)` when it found nothing to act on.
    pub(crate) fn key_more(
        &mut self,
        g: &Geometry,
        name: &str,
        args: &Map<String, Value>,
    ) -> Option<bool> {
        Some(match name {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            "track.solo.clear" | "track.mute.clear" => self.arrange_run(name, json!({})).is_some(),
            "select.off-tracks" if !args.contains_key("ids") => self.clips_off_tracks(),
            "select.span" => {
                let way = text(args, "way")?.to_string();
                self.select_span(&way)
            }
            "take.next" | "take.prev" if !args.contains_key("id") => self.step_take(name),
            "view.page" => {
                let down = match text(args, "way") {
                    Some("down") => true,
                    Some("up") => false,
                    _ => return Some(false),
                };
                self.page(g, down)
            }
            "view.focus" => self.focus_step(g, text(args, "way") != Some("prev")),
            "view.fullscreen" => {
                self.window_ask = Some(WindowAsk::Fullscreen);
                true
            }
            "app.quit" => {
                self.window_ask = Some(WindowAsk::Quit);
                true
            }
            "marker.add.named" => {
                self.popup = None;
                let at = self.playhead;
                self.open_palette_form("marker.add", json!({ "at": at }))
            }
            "transport.locate" => {
                self.popup = None;
                let at = self.project().time_map().format(self.playhead);
                self.open_form_at("transport.seek", &at)
            }
            "range.from-clips" => self.range_from_clips(),
            "browser.search" => self.browser_search(g),
            "browser.tab" => {
                let Some(tab) = text(args, "tab").and_then(RailTab::from_name) else {
                    return Some(false);
                };
                self.press(g, crate::studio::Hit::Rail(tab))
            }
            "view.browser.sort" => self.browser_sort(g, args),
            "view.cleanup" => self.open_cleanup(),
            "mixer.bank" => {
                let next = match text(args, "way") {
                    Some("next") => true,
                    Some("prev") => false,
                    _ => return Some(false),
                };
                self.mixer_bank(g, next)
            }
            "transport.panic" => self.panic(),
            "view.pc-keyboard" => {
                if self.pc_keys.on {
                    self.pc_keys_off();
                } else {
                    self.pc_keys.on = true;
                }
                true
            }
            // The table's commands without the target they need: the selection's, or a form.
            "track.reveal" if !args.contains_key("id") => {
                self.popup = None;
                let seed = match text(args, "in") {
                    Some(place) => json!({ "in": place }),
                    None => json!({}),
                };
                self.open_palette_form("track.reveal", seed)
            }
            "plugin.editor.open" if args.is_empty() => self.open_channel_editor(),
            "transport.goto" if !args.contains_key("to") => {
                self.popup = None;
                self.open_form_at("transport.goto", "marker-1")
            }
            "clip.add.midi" if args.is_empty() => self.add_part(),
            _ => return None,
        })
    }

    /// A command's form with its first field filled in (a seed would leave the field out).
    fn open_form_at(&mut self, command: &'static str, value: &str) -> bool {
        if !self.open_palette_form(command, json!({})) {
            return false;
        }
        if let Some(palette) = &mut self.palette
            && let Some(field) = palette.form.as_mut().and_then(|f| f.fields.first_mut())
        {
            field.value = value.to_string();
            palette.query = value.to_string();
        }
        true
    }

    /// The track the selected row or the selected clip is on.
    fn chosen_track(&self) -> Option<TrackId> {
        match self.selected_row {
            Some(RowKind::Track(t)) => Some(t),
            _ => self.selected_clip.map(|(t, _)| t),
        }
    }

    /// The selected tracks' clips leave the selection (Deselect All on Tracks).
    fn clips_off_tracks(&mut self) -> bool {
        let mut tracks: Vec<String> = self
            .arrange
            .tracks
            .iter()
            .filter_map(|r| match r {
                RowKind::Track(t) => Some(t.to_string()),
                _ => None,
            })
            .collect();
        if tracks.is_empty() {
            tracks.extend(self.chosen_track().map(|t| t.to_string()));
        }
        let clips = self.picked_clips();
        self.give_selection(&[], &clips);
        let done = self
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            .arrange_run("select.off-tracks", json!({ "ids": tracks }))
            .is_some();
        self.take_clips();
        done
    }

    /// The selection grows to the song's start or end on each track (Select from Start to Event).
    fn select_span(&mut self, way: &str) -> bool {
        let clips = self.picked_clips();
        self.give_selection(&[], &clips);
        let done = self
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            .arrange_run("select.span", json!({ "way": way }))
            .is_some();
        self.take_clips();
        done
    }

    /// The next or the previous take plays (Replace Event With Next Layer).
    fn step_take(&mut self, name: &str) -> bool {
        let Some(id) = self
            .selected_clip
            .map(|(_, c)| c)
            .or_else(|| self.picked_clips().first().copied())
        else {
            return false;
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.arrange_run(name, json!({ "id": id.to_string() }))
            .is_some()
    }

    /// Scrolls the view in front (the roll, or the arrangement) by its height.
    fn page(&mut self, g: &Geometry, down: bool) -> bool {
        let sign = if down { 1.0 } else { -1.0 };
        if matches!(self.input_context(g), "roll" | "lane")
            && let Some(ctx) = self.roll_ctx(g)
        {
            let before = self.roll.scroll.1;
            let h = self.roll.view(&ctx).grid.h;
            self.roll.scroll.1 += sign * h;
            self.roll.clamp_scroll(&ctx);
            return self.roll.scroll.1 != before;
        }
        let before = self.scroll.1;
        self.scroll.1 += sign * g.lanes.h;
        self.clamp_scroll(g);
        self.arrange.touched();
        self.scroll.1 != before
    }

    /// The part of the window the keys go to now.
    pub(crate) fn place(&self) -> Place {
        if self.browser.open && self.input_map.browser_focus {
            Place::Browser
        } else if !self.arrange.focus && self.drawer_open {
            Place::Dock
        } else {
            Place::Arrange
        }
    }

    /// The keys go to the next (or the previous) part that is shown.
    fn focus_step(&mut self, g: &Geometry, forward: bool) -> bool {
        let mut shown = vec![Place::Arrange];
        if self.drawer_open {
            shown.push(Place::Dock);
        }
        if self.browser.open {
            shown.push(Place::Browser);
        }
        let n = shown.len();
        let i = shown.iter().position(|p| *p == self.place()).unwrap_or(0);
        let to = shown[if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        }];
        self.input_map.browser_focus = to == Place::Browser;
        self.arrange.focus = to == Place::Arrange;
        self.browser.typing = false;
        self.rebuild(g);
        true
    }

    /// The selected clips' times become the range (Create Ranges).
    fn range_from_clips(&mut self) -> bool {
        let p = self.project();
        let mut spans: Vec<(Tick, Tick)> = self
            .arrange
            .clips
            .iter()
            .filter_map(|(_, c)| p.clip(*c).ok())
            .map(|(_, c)| (c.start, c.start + c.length))
            .collect();
        if spans.is_empty() {
            return false;
        }
        spans.sort_unstable();
        let mut merged: Vec<(Tick, Tick)> = Vec::new();
        for (s, e) in spans {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        let start = merged.first().map_or(0, |r| r.0);
        let end = merged.last().map_or(0, |r| r.1);
        self.arrange.range = Some((start, end));
        self.arrange.ranges = merged;
        self.arrange.clips.clear();
        self.selected_clip = None;
        self.give_selection(&[], &[]);
        self.arrange.touched();
        true
    }

    /// The browser's search field takes the keys, the browser opened if it is shut.
    fn browser_search(&mut self, g: &Geometry) -> bool {
        if !self.browser.open {
            let tab = self.browser.tab;
            self.browser.press_rail(tab);
        }
        self.browser.typing = true;
        self.input_map.browser_focus = true;
        self.rebuild(g);
        true
    }

    /// The mixer scrolls by as many whole strips as it shows (Next Bank).
    fn mixer_bank(&mut self, g: &Geometry, next: bool) -> bool {
        if !(self.drawer_open && self.tab == DrawerTab::Mixer) {
            return false;
        }
        let w = crate::mixer::STRIP_W
            * if self.song.session().state.view.mixer_wide {
                1.4
            } else {
                1.0
            }
            + crate::mixer::STRIP_GAP;
        let page = ((self.mixer_view(g).w / w).floor() * w).max(w);
        let before = self.mixer.scroll;
        let to = if next { before + page } else { before - page };
        // Whole strips: the bank starts at a strip's left edge.
        self.mixer.scroll = ((to / w).round() * w).clamp(0.0, self.mixer_max_scroll(g));
        self.mixer.scroll != before
    }

    /// Every note that sounds ends: the keys held, the roll's, and what the player plays.
    fn panic(&mut self) -> bool {
        self.pc_keys_release();
        let at = self.playhead;
        // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
        self.seek(at);
        true
    }

    /// The window of the selected track's instrument, or of its first effect (Show Channel
    /// Editor). A second press shuts it.
    fn open_channel_editor(&mut self) -> bool {
        let Some(t) = self.chosen_track() else {
            return false;
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let id: Option<PluginId> = crate::instrument::current(track)
            .or_else(|| track.inserts.first())
            .map(|x| x.id);
        match id {
            Some(id) => self.open_plugin(t, id),
            None => false,
        }
    }

    /// An empty MIDI clip on the selected instrument track: over the loop when there is one,
    /// else the bar the playhead is in (Insert Instrument Part).
    fn add_part(&mut self) -> bool {
        let Some(t) = self.chosen_track() else {
            return false;
        };
        let p = self.project();
        if p.tracks.get(&t).map(|x| x.kind) != Some(TrackKind::Instrument) {
            return false;
        }
        let (at, length) = match p.loop_range {
            Some(l) if l.end > l.start => (l.start, l.end - l.start),
            _ => {
                let map = p.time_map();
                let (bar, _, _) = map.to_bbt(self.playhead);
                let at = map.bar_start(bar);
                (at, map.bar_start(bar + 1) - at)
            }
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        let Some(data) = self.arrange_run(
            "clip.add.midi",
            json!({ "track": t.to_string(), "at": at, "length": length }),
        ) else {
            return false;
        };
        let made: Option<ClipId> = data["id"].as_str().and_then(|s| s.parse().ok());
        if let Some(c) = made {
            self.select_made(&json!({ "ids": [c.to_string()] }));
        }
        true
    }

    /// The screen's commands the session kept for the window (an AI's over MCP), run here.
    pub(crate) fn run_asked(&mut self, g: &Geometry) -> bool {
        let asked = self.song.asked().clone();
        let list = {
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            let mut a = asked.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut a.list)
        };
        if list.is_empty() {
            return false;
        }
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        asked.lock().unwrap_or_else(|e| e.into_inner()).busy = true;
        for (name, args) in list {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.run_parsed(g, name, args);
        }
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        asked.lock().unwrap_or_else(|e| e.into_inner()).busy = false;
        true
    }

    /// `view.browser.sort`: the order of a tab's list (the shown tab's when it names none),
    /// showing that tab. An order the tab does not have says so.
    fn browser_sort(&mut self, g: &Geometry, args: &Map<String, Value>) -> bool {
        let tab = match text(args, "tab") {
            Some(t) => match RailTab::from_name(t) {
                Some(tab) => tab,
                None => return false,
            },
            None => self.browser.tab,
        };
        let Some(order) = text(args, "order") else {
            return false;
        };
        let Some(view) = View::of(tab).iter().copied().find(|v| v.name() == order) else {
            let how = View::ALL
                .iter()
                .find(|v| v.name() == order)
                .map_or(order, |v| v.label());
            self.error = Some(format!("{}の一覧は{how}に並べ替えできません", tab.label()));
            return false;
        };
        self.popup = None;
        if !self.browser.open || self.browser.tab != tab {
            self.browser.press_rail(tab);
            self.rebuild(g);
        }
        self.browser.set_view(view);
        true
    }

    /// Whether the session kept a screen command for the window.
    pub(crate) fn has_asked(&self) -> bool {
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let asked = self.song.asked().lock();
        !asked.unwrap_or_else(|e| e.into_inner()).list.is_empty()
    }
}
