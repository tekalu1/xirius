//! ARA on the clips (EDT-023, PLG-036, PLG-055, PLG-058; the approved screens of 2026-11-03):
//! the rows of the clip's menu, the list that picks the plugin, the badge after the clip's
//! name, and the plugin's own window.
//!
//! - 「処理」 ends with 「ARA プラグインを追加…」 (Ctrl+M). It opens the list menu of the ARA
//!   plugins; picking one attaches it, even when there is only one. A clip with changed tempo,
//!   speed, pitch or direction, or on a frozen track, has the row dim with its reason.
//! - An attached clip has 「〈名前〉で編集」 and 「〈名前〉を外す」 (Ctrl+Alt+M) there instead, and
//!   a badge after its name: a turning ring and the plugin's percent while it analyses (the
//!   ring alone while it renders, or when the clip is too narrow), the plugin's mark when done,
//!   the same mark in the warning colour when the plugin went down, and the mark struck through
//!   when it cannot be opened.
//! - A plugin that went down says so only by the colour (PLG-055): a press on the badge opens a
//!   small menu of the plugin's name, 「編集」 (dim while it is down), 「再起動」 and 「外す」.
//! - A press on the badge, a double click on the clip or 「〈名前〉で編集」 opens the plugin's
//!   window, one per plugin; when it closes, the plugin's edits are taken into the song.

use serde_json::{Map, Value, json};
use xirius_core::commands::clip_ara::{Unfit, unfit_all};
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::{Clip, Track};
use xirius_design::{FontSize, Radius, Space, Weight};
use xirius_session::ara::{Phase, Report};
use xirius_session::output::OutputMode;

use crate::draw::{Align, Pen};
use crate::popup::{Action, PopupKind};
use crate::studio::{Geometry, Rect, Studio};
use crate::toast::Toast;

/// The badge's box: the icon's room and the height of the name's line.
pub const BOX_W: f32 = 20.0;
pub const BOX_H: f32 = 18.0;
const ICON: f32 = 12.0;
/// Between the name and the badge.
pub const GAP: f32 = 2.0;
pub const ADD: &str = "ARA プラグインを追加…";
pub const MISSING: &str = "プラグインが見つかりません";
pub const RESTART: &str = "プラグインを再起動";
/// Why 「編集」 is dim while the plugin is down.
pub const DOWN: &str = "プラグインが停止しています";
/// Why nothing is analysed without the audio device: the plugins run in its host.
pub const NO_HOST: &str = "音声の装置が開いていないため、プラグインを開けません";
const NOT_ANALYSED: &str = "解析できませんでした";
// design-lint: allow(guidance) 開けない理由のエラー。いつ開けるかを言わないと次が分からない
const NOT_READY: &str = "プラグインの準備ができていません。解析が始まってから開いてください";
const NO_MATCH: &str = "一致するプラグインはありません";
const NONE_FOUND: &str = "ARA に対応したプラグインが見つかりません";

/// What the screen keeps of ARA between frames.
#[derive(Default)]
pub struct AraUi {
    /// The clips the open list attaches its plugin to.
    pub pick: Vec<ClipId>,
    /// The plugins' state, read at the last poll.
    pub reports: Vec<Report>,
    /// The plugins whose window was open at the last poll.
    pub editing: Vec<String>,
    /// The tests' plugins, in place of the host's.
    pub fake: Option<Vec<Report>>,
    /// The windows asked for without a host (the tests): the plugin and its clips.
    pub opened: Vec<(String, Vec<ClipId>)>,
}

/// What the badge after an attached clip's name shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Badge {
    /// Analysing: the plugin's progress over its clips, in percent, when it has one (not while
    /// it renders or opens).
    Busy(Option<u8>),
    Ready,
    /// The plugin went down (PLG-055).
    Crashed,
    /// It cannot be opened, and why.
    Failed(String),
}

impl Badge {
    /// What a screen reader says of it.
    pub fn said(&self) -> String {
        match self {
            Badge::Busy(Some(n)) => format!("解析中 {n}%"),
            Badge::Busy(None) => "解析中".into(),
            Badge::Ready => "済み".into(),
            Badge::Crashed => "停止".into(),
            Badge::Failed(_) => "開けません".into(),
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Badge::Failed(why) => Some(why),
            _ => None,
        }
    }

    /// The width of its box when there is room; a busy badge has its percent after the ring.
    pub fn width(&self) -> f32 {
        match self {
            Badge::Busy(Some(n)) => {
                BOX_W + crate::fonts::measure(&format!("{n}%"), FontSize::Micro, Weight::Semibold)
            }
            _ => BOX_W,
        }
    }
}

fn percent(f: f32) -> u8 {
    (f.clamp(0.0, 1.0) * 100.0).floor() as u8
}

/// The ids of `--ids` that are clips.
fn clip_ids(args: &Map<String, Value>) -> Vec<ClipId> {
    match args.get("ids") {
        Some(Value::Array(xs)) => xs.iter().filter_map(|x| x.as_str()?.parse().ok()).collect(),
        Some(Value::String(s)) => s.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
        _ => Vec::new(),
    }
}

impl Studio {
    fn ara_reports(&self) -> &[Report] {
        self.ara.fake.as_deref().unwrap_or(&self.ara.reports)
    }

    fn ara_report(&self, plugin: &str) -> Option<&Report> {
        self.ara_reports()
            .iter()
            .find(|r| r.plugin.eq_ignore_ascii_case(plugin))
    }

    /// The name the rows call the plugin of an attached clip: the catalog's, else the one kept
    /// with the clip.
    pub fn ara_name(&self, clip: ClipId) -> Option<String> {
        let p = self.project();
        let a = p.ara.clips.get(&clip)?;
        let found = self
            .mixer
            .catalog
            .as_deref()
            .and_then(|c| xirius_session::ara::find(c, &a.plugin))
            .map(|(_, name)| name);
        Some(found.unwrap_or_else(|| {
            if a.name.is_empty() {
                a.plugin.clone()
            } else {
                a.name.clone()
            }
        }))
    }

    /// The badge of `clip`; none when no plugin is attached to it.
    pub fn ara_badge(&self, clip: ClipId) -> Option<Badge> {
        let p = self.project();
        let a = p.ara.clips.get(&clip)?;
        if self.ara.fake.is_none()
            && let Some(catalog) = self.mixer.catalog.as_deref()
            && xirius_session::ara::find(catalog, &a.plugin).is_none()
        {
            return Some(Badge::Failed(MISSING.into()));
        }
        let Some(r) = self.ara_report(&a.plugin) else {
            // Not read yet: the document is being opened, or opens with the player (the song
            // is being prepared, or no play wanted the player yet). Never 「済み」 without a
            // report: the clip plays its own sound.
            return Some(if self.ara_without_host() {
                Badge::Failed(NO_HOST.into())
            } else {
                Badge::Busy(None)
            });
        };
        // Down before cannot be opened: a restart may open it again.
        if r.crashed.is_some() {
            return Some(Badge::Crashed);
        }
        if let Some(why) = &r.failed {
            return Some(Badge::Failed(why.clone()));
        }
        Some(
            match r
                .phases
                .iter()
                .find(|(k, _)| *k == clip.0)
                .map(|(_, ph)| *ph)
            {
                Some(Phase::Ready) => Badge::Ready,
                Some(Phase::Failed) => Badge::Failed(NOT_ANALYSED.into()),
                Some(Phase::Crashed) => Badge::Crashed,
                // Analysed, being rendered: the percent would stay at 100.
                Some(Phase::Rendering) => Badge::Busy(None),
                _ => Badge::Busy(Some(percent(r.progress))),
            },
        )
    }

    /// No player will come to open the documents: nothing plays here, the sound device cannot
    /// be had, or the player did not start. Not while the song is being prepared, nor before a
    /// play first wants the player.
    fn ara_without_host(&self) -> bool {
        let t = &self.transport;
        self.ara.fake.is_none()
            && t.host().is_none()
            && !t.preparing()
            && (t.mode == OutputMode::None || t.down.is_some() || t.start_failed)
    }

    /// Where the badge of `clip` (at `r`) sits: right after its name, before the marks, and at
    /// the name's start when the clip is too narrow for the name. It stays inside the clip: a
    /// busy badge without the room drops its percent, and the box is cut at the clip's end.
    pub fn ara_badge_rect(&self, track: &Track, clip: &Clip, r: Rect, badge: &Badge) -> Rect {
        let pad = Space::X1.px();
        let mut name_x = r.x + pad;
        if crate::draw::clip_badge(self, track.id).is_some() {
            name_x += crate::draw::CLIP_BADGE + pad;
        }
        let name_w = crate::fonts::measure(&clip.name, FontSize::Caption, Weight::Medium);
        let marks = crate::clip_marks::marks(track, clip);
        let end = if marks.is_empty() {
            r.right()
        } else {
            crate::clip_marks::marks_left(&marks, r, name_w, ara_reserve()) - 2.0
        };
        let end = end.min(r.right());
        let full = badge.width();
        let w = if end - name_x >= full { full } else { BOX_W };
        let x = (name_x + name_w + GAP)
            .min(end - w)
            .max(name_x)
            .min(r.right() - w)
            .max(r.x);
        Rect::new(x, r.y, w.min(r.right() - x), BOX_H)
    }

    /// The badge's box of an attached clip on screen, by its track and id.
    pub fn ara_badge_at(&self, g: &Geometry, track: TrackId, clip: ClipId) -> Option<Rect> {
        let badge = self.ara_badge(clip)?;
        let row = self
            .tree()
            .row_of(crate::tree::RowKind::Track(track))
            .copied()?;
        let p = self.project();
        let t = p.tracks.get(&track)?;
        let c = t.clips.iter().find(|c| c.id == clip)?;
        let r = self.clip_rect(g, &row, c);
        let at = self.ara_badge_rect(t, c, r, &badge);
        let x = at.x.max(r.x);
        let w = at.right().min(r.right()) - x;
        let h = at.bottom().min(r.bottom()) - at.y;
        (w > 0.0 && h > 0.0).then(|| Rect::new(x, at.y, w, h))
    }

    /// Between frames: reads the plugins' state, takes the edits of a window that closed into
    /// the song, and says what a window could not open. True while a badge's ring turns.
    pub fn poll_ara(&mut self) -> bool {
        if self.ara.fake.is_none() {
            let Some(host) = self.transport.host() else {
                return false;
            };
            let attached = !self.project().ara.clips.is_empty();
            if !attached && self.ara.editing.is_empty() && self.ara.reports.is_empty() {
                return false;
            }
            self.ara.reports = host.ara_reports();
            let editing = host.ara_editing();
            let errors = host.take_ara_editor_errors();
            let pending = host.take_ara_arrived();
            let closed = self.ara.editing.iter().any(|p| !editing.contains(p));
            self.ara.editing = editing;
            if closed {
                self.pull_plugins();
            }
            if let Some((_, why)) = errors.first() {
                self.warn(format!("プラグインの窓を開けません（{why}）"), None);
            }
            // An archive on its way comes in at a later frame: keep them coming.
            if pending {
                return true;
            }
        }
        let p = self.project();
        p.ara
            .clips
            .keys()
            .any(|c| matches!(self.ara_badge(*c), Some(Badge::Busy(_))))
    }

    /// The tests' restart of the plugins that went down.
    pub(crate) fn ara_restarted(&mut self) {
        if let Some(fake) = &mut self.ara.fake {
            for r in fake.iter_mut() {
                r.crashed = None;
            }
        }
    }

    /// The clips a command acts on: its `--ids`, else the selected clips, with their tracks.
    pub(crate) fn ara_targets(&self, args: &Map<String, Value>) -> Vec<(TrackId, ClipId)> {
        let p = self.project();
        let ids = clip_ids(args);
        if ids.is_empty() {
            return self.arrange.clips.iter().copied().collect();
        }
        ids.into_iter()
            .filter_map(|c| p.clip(c).ok().map(|(t, _)| (t.id, c)))
            .collect()
    }

    /// `clip.ara.attach` without a plugin (Ctrl+M, the menu's row): the list that picks the
    /// plugin, or the plugin's window when the clip has one.
    pub(crate) fn ara_attach_command(&mut self, g: &Geometry, args: &Map<String, Value>) -> bool {
        let targets = self.ara_targets(args);
        if targets.is_empty() {
            // design-lint: allow(guidance) 対象の無い操作のエラー。何が足りないかを言う
            self.error = Some("ARA のプラグインを付けるクリップを選んでください".into());
            return false;
        }
        let p = self.project();
        if let Some(plugin) = targets
            .iter()
            .find_map(|(_, c)| p.ara.clips.get(c))
            .map(|a| a.plugin.clone())
        {
            let clips: Vec<ClipId> = targets
                .iter()
                .map(|(_, c)| *c)
                .filter(|c| p.ara.clips.get(c).is_some_and(|a| a.plugin == plugin))
                .collect();
            // Some of the clips have the plugin: its window shows those, and says the rest
            // are left as they are (L4).
            let rest = targets.len() - clips.len();
            let opened = self.open_ara_editor_for(&clips);
            if opened && rest > 0 {
                self.ara_tell(format!("ほかのクリップ {rest} 件は窓に出していません"));
            }
            return opened;
        }
        // The menu's reason, word for word (EDT-023).
        let clips: Vec<ClipId> = targets.iter().map(|(_, c)| *c).collect();
        if let Some(why) = unfit_all(&p, &clips) {
            self.error = Some(why.said().into());
            return false;
        }
        self.ara.pick = targets.iter().map(|(_, c)| *c).collect();
        let (t, c) = targets[0];
        let anchor = self
            .tree()
            .row_of(crate::tree::RowKind::Track(t))
            .copied()
            .and_then(|row| {
                let clip = p.tracks.get(&t)?.clips.iter().find(|k| k.id == c)?.clone();
                let r = self.clip_rect(g, &row, &clip);
                Some(Rect::new(r.x, r.y, r.w.min(260.0), BOX_H))
            })
            .filter(|r| g.lanes.contains(r.x.max(g.lanes.x), r.y.max(g.lanes.y)))
            .unwrap_or(Rect::new(g.lanes.x + 8.0, g.lanes.y + 8.0, 1.0, BOX_H));
        if self
            .popup
            .as_ref()
            .is_some_and(|p| p.kind == PopupKind::AraPlugin)
        {
            self.popup = None;
        }
        self.open_popup(PopupKind::AraPlugin, anchor);
        true
    }

    /// `clip.ara.detach` without ids: the selected clips that have a plugin.
    pub(crate) fn ara_detach_selected(&mut self) -> bool {
        let p = self.project();
        let ids: Vec<String> = self
            .arrange
            .clips
            .iter()
            .filter(|(_, c)| p.ara.clips.contains_key(c))
            .map(|(_, c)| c.to_string())
            .collect();
        if ids.is_empty() {
            // design-lint: allow(guidance) 対象の無い操作のエラー。何が足りないかを言う
            self.error = Some("ARA のプラグインが付いたクリップを選んでください".into());
            return false;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.arrange_run("clip.ara.detach", json!({ "ids": ids }))
            .is_some()
    }

    /// The list menu of the ARA plugins, under their makers' headings.
    pub(crate) fn ara_pick_list(&mut self) -> crate::pick_list::PickList {
        use crate::pick_list::{PickGroup, PickList, PickRow};
        let catalog = self.plugin_catalog();
        let mut groups: Vec<PickGroup> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for (uid, name) in xirius_session::ara::plugins(&catalog) {
            // A plugin registered and in the catalog is one row.
            if seen.iter().any(|u| u.eq_ignore_ascii_case(&uid)) {
                continue;
            }
            seen.push(uid.clone());
            let vendor = catalog
                .entries
                .iter()
                .find(|e| e.uid.eq_ignore_ascii_case(&uid))
                .map(|e| e.vendor.clone())
                .unwrap_or_default();
            let row = PickRow::new(name).icon("ara").act(Action::Pick(uid));
            match groups.iter_mut().find(|g| g.heading == vendor) {
                Some(g) => g.rows.push(row),
                None => groups.push(PickGroup::new(vendor, vec![row])),
            }
        }
        // The makers in order, the plugins without one first.
        groups.sort_by_key(|g| g.heading.to_lowercase());
        let mut list = PickList::of(Vec::new()).named(NO_MATCH);
        list.icon = Some("ara");
        if groups.is_empty() {
            list.groups = vec![PickGroup::new("", vec![PickRow::new(NONE_FOUND).note()])];
        } else {
            list.groups = groups;
        }
        list.footer = self.scan_footer();
        list
    }

    /// A plugin picked in the list: it is attached to the clips the list was opened for.
    pub(crate) fn ara_picked(&mut self, to: &str) {
        if to == crate::mixer::scan::RESCAN {
            if let Err(e) = self.start_plugin_scan() {
                self.error = Some(e);
            }
            return;
        }
        if to == crate::mixer::scan::STOP {
            self.stop_plugin_scan();
            return;
        }
        let ids: Vec<String> = std::mem::take(&mut self.ara.pick)
            .iter()
            .map(ToString::to_string)
            .collect();
        if ids.is_empty() {
            return;
        }
        self.ara_attach_to(&ids, to);
    }

    /// `clip.ara.attach` with its plugin. The name is the window's own catalog's, so the step
    /// is the core's `clip.ara.attach.apply` (the session's command is the song owner's, which
    /// the window is not).
    pub(crate) fn ara_attach_to(&mut self, ids: &[String], plugin: &str) -> bool {
        let Some((plugin, name)) = xirius_session::ara::find(&self.plugin_catalog(), plugin) else {
            self.error = Some(format!("ARA のプラグイン {plugin} が見つかりません"));
            return false;
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.arrange_run(
            "clip.ara.attach.apply",
            json!({ "ids": ids, "plugin": plugin, "name": name }),
        )
        .is_some()
    }

    /// Opens the window of the plugin attached to `clips` (one window per plugin, used again),
    /// showing those clips.
    pub(crate) fn open_ara_editor_for(&mut self, clips: &[ClipId]) -> bool {
        let Some(first) = clips.first() else {
            return false;
        };
        let p = self.project();
        let Some(a) = p.ara.clips.get(first) else {
            return false;
        };
        let plugin = a.plugin.clone();
        let name = self.ara_name(*first).unwrap_or_else(|| plugin.clone());
        match self.ara_badge(*first) {
            Some(Badge::Failed(why)) => {
                self.ara_tell(why);
                return false;
            }
            // Its document is gone with the plugin: restart it first.
            Some(Badge::Crashed) => {
                self.error = Some(DOWN.into());
                return false;
            }
            _ => {}
        }
        let place = self.editor_placement();
        let Some(host) = self.transport.host() else {
            self.ara.opened.push((plugin, clips.to_vec()));
            return true;
        };
        let title = format!("{name} · ARA");
        match host.open_ara_editor(&plugin, &title, place, clips) {
            Ok(()) => true,
            Err(e) if e.contains("not open") => {
                self.warn(NOT_READY.into(), None);
                false
            }
            Err(e) => {
                self.warn(format!("{name} の窓を開けません（{e}）"), None);
                false
            }
        }
    }

    /// Says why a badge cannot be opened, in a brief note.
    fn ara_tell(&mut self, why: String) {
        self.toast = Some(Toast {
            info: true,
            brief: true,
            ..Toast::plain(why)
        });
    }

    /// A press on the badge of `clip`: its window, why it cannot be opened, or (the plugin went
    /// down) the small menu of the plugin.
    pub(crate) fn ara_badge_press(&mut self, g: &Geometry, track: TrackId, clip: ClipId) -> bool {
        match self.ara_badge(clip) {
            None => false,
            Some(Badge::Crashed) => {
                let Some(at) = self.ara_badge_at(g, track, clip) else {
                    return false;
                };
                self.select_clip_here(track, clip);
                let items = self.ara_down_menu(track, clip);
                self.open_context(items, at.x, at.bottom());
                true
            }
            Some(Badge::Failed(why)) => {
                self.select_clip_here(track, clip);
                self.ara_tell(why);
                true
            }
            Some(_) => {
                self.select_clip_here(track, clip);
                self.open_ara_editor_for(&[clip])
            }
        }
    }

    /// The menu of the badge of a plugin that went down (PLG-055): its name, 「編集」 (dim),
    /// 「再起動」 and 「外す」.
    pub(crate) fn ara_down_menu(&self, track: TrackId, clip: ClipId) -> Vec<crate::context::Item> {
        use crate::context::{Act, Item};
        let name = self.ara_name(clip).unwrap_or_default();
        let frozen = self
            .project()
            .tracks
            .get(&track)
            .is_some_and(|t| t.frozen.is_some());
        let edit = Item::act("編集", "", Act::AraAttach(vec![clip.to_string()]));
        vec![
            Item::title(&name),
            Item {
                reason: DOWN.into(),
                ..edit.enabled(false)
            },
            Item::act("再起動", "", Act::RestartPlugins),
            self.ctx_session(
                "外す",
                "clip.ara.detach",
                "clip.ara.detach",
                json!({ "ids": [clip.to_string()] }),
                !frozen,
                crate::toast::FROZEN,
            ),
        ]
    }

    /// The rows at the end of 「処理」 for the clips `ids`: attach, or edit and detach.
    pub(crate) fn ara_rows(&self, ids: &[String], frozen: bool) -> Vec<crate::context::Item> {
        use crate::context::{Act, Item};
        let p = self.project();
        let clips: Vec<ClipId> = ids.iter().filter_map(|i| i.parse().ok()).collect();
        let attached = clips.iter().find(|c| p.ara.clips.contains_key(c)).copied();
        let Some(one) = attached else {
            let reason = unfit_all(&p, &clips).map_or("", Unfit::said);
            let row = Item::act(
                ADD,
                &self.menu_key("clip.ara.attach"),
                Act::AraAttach(ids.to_vec()),
            );
            return vec![Item {
                reason: reason.to_string(),
                ..row.enabled(reason.is_empty())
            }];
        };
        let name = self.ara_name(one).unwrap_or_default();
        let with: Vec<String> = clips
            .iter()
            .filter(|c| p.ara.clips.contains_key(c))
            .map(ToString::to_string)
            .collect();
        let down = clips
            .iter()
            .any(|c| self.ara_badge(*c) == Some(Badge::Crashed));
        let edit = Item::act(
            &format!("{name}で編集"),
            &self.menu_key("clip.ara.attach"),
            Act::AraAttach(with.clone()),
        );
        vec![
            Item {
                reason: if down { DOWN.into() } else { String::new() },
                ..edit.enabled(!down)
            },
            self.ctx_session(
                &format!("{name}を外す"),
                "clip.ara.detach",
                "clip.ara.detach",
                json!({ "ids": with }),
                !frozen,
                crate::toast::FROZEN,
            ),
        ]
    }

    /// The row that restarts the plugins that went down, for the head of the menu of clips one
    /// of whose plugins stopped (PLG-055).
    pub(crate) fn ara_restart_row(&self, ids: &[String]) -> Option<crate::context::Item> {
        use crate::context::{Act, Item};
        let crashed = ids
            .iter()
            .filter_map(|i| i.parse::<ClipId>().ok())
            .any(|c| self.ara_badge(c) == Some(Badge::Crashed));
        crashed.then(|| Item::act(RESTART, "", Act::RestartPlugins))
    }

    /// The hover balloon of the badge: what it is and, when it cannot be opened, why.
    pub fn ara_tip(&self, clip: ClipId) -> Option<String> {
        let badge = self.ara_badge(clip)?;
        let name = self.ara_name(clip)?;
        Some(match badge {
            Badge::Busy(Some(n)) => format!("{name}: 解析中 {n}%"),
            Badge::Busy(None) => format!("{name}: 解析中"),
            Badge::Ready => format!("{name}で編集"),
            Badge::Crashed => format!("{name}: 停止"),
            Badge::Failed(why) => format!("{name}: {why}"),
        })
    }
}

/// The room the marks leave the badge: its widest (a busy badge at 100%) and its gap.
pub fn ara_reserve() -> f32 {
    Badge::Busy(Some(100)).width() + GAP
}

/// Draws the badge in its box `at`, lit under the pointer; a busy badge has its percent when the
/// box has the room for it.
pub fn paint_badge(pen: &mut Pen, badge: &Badge, at: Rect, hot: bool) {
    let c = pen.c;
    if hot {
        pen.round(at, c.clip_sel, Radius::Xs);
    }
    let icon = Rect::new(
        at.x + (BOX_W - ICON) / 2.0,
        at.y + (BOX_H - ICON) / 2.0,
        ICON,
        ICON,
    );
    match badge {
        Badge::Ready => pen.icon("ara", icon, if hot { c.ink_strong } else { c.ink }),
        Badge::Crashed => pen.icon("ara", icon, c.ink_alert),
        Badge::Failed(_) => pen.icon("ara-off", icon, if hot { c.ink_strong } else { c.ink }),
        Badge::Busy(n) => {
            let centre = (at.x + BOX_W / 2.0, at.y + BOX_H / 2.0);
            crate::clip_marks::spinner(pen, centre);
            let Some(n) = n.filter(|_| at.w + 0.5 >= badge.width()) else {
                return;
            };
            let text = format!("{n}%");
            pen.text(
                &text,
                FontSize::Micro,
                Weight::Semibold,
                c.ink,
                Rect::new(at.x + BOX_W - 2.0, at.y, at.w - BOX_W + 2.0, BOX_H),
                Align::Left,
            );
        }
    }
}

#[cfg(test)]
#[path = "clip_ara_tests.rs"]
mod tests;
