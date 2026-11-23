//! A VST3 plugin's own window (UI-040, PLG-027).
//!
//! The window shows the instance that plays, so what is turned in it is heard at once and,
//! once let go, becomes one step of the history ([`Studio::poll_editors`]):
//!
//! A plugin host process opens the window of its instance. Studio-window activation queues
//! a raise without assigning a cross-process HWND owner or taking focus.
//!
//! The player starts when a window needs it. Under tests, and while
//! [`xirius_vst3::OFFSCREEN_ENV`] is set, windows open off every screen.
//!

use xirius_core::id::PluginId;
use xirius_core::model::Plugin;
use xirius_engine::server::EditorOpenEvent;
use xirius_vst3::{Caption, Placement};

use crate::studio::Studio;

/// Where an open window's instance is.
pub(crate) enum Where {
    /// The plugin host process's instance; the host has the window. `opened` says the host has
    /// said the window is open once, so it is no longer taken as open while it does not answer.
    Host { opened: bool },
}

/// A plugin's own window.
pub(crate) struct OpenEditor {
    pub id: PluginId,
    pub title: String,
    pub at: Where,
    pub foreground_from: u64,
    pub retired_loading: Option<u64>,
}

impl OpenEditor {
    pub(crate) fn awaits_placement(&self) -> bool {
        self.foreground_from != 0
            && self.retired_loading.is_none()
            && matches!(self.at, Where::Host { opened: true })
    }

    pub fn param_titles(&self) -> Vec<(u32, String)> {
        Vec::new()
    }

    /// The instance plays: what is turned in the window is heard.
    pub fn heard(&self) -> bool {
        true
    }
}

/// Why a plugin's window does not open, in the words the screen shows (ARC-027).
fn cannot(name: &str, why: &str) -> String {
    format!("{name} {why}ため、窓を開けません")
}

pub(crate) fn editor_touch_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

impl Studio {
    pub(crate) fn editor_foreground() -> u64 {
        #[cfg(windows)]
        if !cfg!(test) && std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_none() {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowThreadProcessId,
            };
            let mut process = 0;
            // SAFETY: these queries do not send messages to another GUI thread.
            let foreground = unsafe {
                let hwnd = GetForegroundWindow();
                GetWindowThreadProcessId(hwnd, &mut process);
                hwnd
            };
            if process == std::process::id() {
                return foreground as u64;
            }
        }
        0
    }

    pub(crate) fn editor_stack(&self) -> Vec<u64> {
        let mut windows: Vec<_> = self
            .mixer
            .editors
            .iter()
            .filter_map(|e| self.mixer.editor_touches.get(&e.id).copied())
            .collect();
        #[cfg(windows)]
        windows.extend(self.ara.editor_windows.values().copied());
        windows.sort_by_key(|&(window, time)| std::cmp::Reverse((time, window)));
        windows.into_iter().map(|(window, _)| window).collect()
    }

    #[cfg(windows)]
    pub(crate) fn sync_ara_editor_windows(&mut self, live: Vec<(String, u64)>) {
        let old = std::mem::take(&mut self.ara.editor_windows);
        let mut opened = false;
        for (plugin, hwnd) in live {
            let state = old.get(&plugin).copied().filter(|(h, _)| *h == hwnd);
            let entry = state.unwrap_or_else(|| {
                opened = true;
                (hwnd, editor_touch_time())
            });
            self.ara.editor_windows.insert(plugin, entry);
        }
        if opened {
            self.raise_editors_over(0);
        }
    }

    #[cfg(windows)]
    pub(crate) fn touch_ara_editor(&mut self, window: u64) {
        for (hwnd, time) in self.ara.editor_windows.values_mut() {
            if *hwnd == window {
                *time = editor_touch_time();
            }
        }
    }

    fn vst_bar(&self, plugin: &Plugin) -> xirius_vst3::EditorBar {
        let p = self.project();
        let track = p
            .tracks
            .values()
            .find(|t| t.inserts.iter().any(|p| p.id == plugin.id));
        xirius_vst3::EditorBar {
            track: track.map_or_else(String::new, |t| t.shown_name().into()),
            device: plugin.name.clone(),
            preset: plugin.preset.clone().unwrap_or_else(|| "—".into()),
            presets: self
                .instruments
                .presets
                .external
                .get(&plugin.uid.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default(),
            menu: track.map_or_else(Vec::new, |t| {
                use crate::instrument::menu::{self, Menu};
                let (rows, current) =
                    menu::view(&p, &self.instruments.presets, Menu::Preset(t.id, plugin.id));
                rows.into_iter()
                    .map(|row| xirius_vst3::PresetRow {
                        enabled: !row.to.starts_with(menu::OFF),
                        selected: row.to == current,
                        footer: row.to.trim_start_matches(menu::OFF).starts_with(menu::FOOT),
                        key: row.to,
                        label: row.label,
                    })
                    .collect()
            }),
            stack: self.editor_stack(),
            raise_ticket: 0,
            stack_base: crate::park::own_handle(crate::screen::WINDOW_TITLE).unwrap_or(0) as u64,
            instrument: plugin.kind == xirius_core::model::PluginKind::Instrument,
            powered: plugin.runs(),
            pinned: self.mixer.editor_pins.contains(&plugin.id),
            sidechain: plugin.sidechain.is_some(),
        }
    }
    /// Reconcile the desktop's owner group after a Studio window is activated.
    #[cfg(windows)]
    pub(crate) fn studio_window_activated(&mut self, window: u64) {
        self.queue_editor_raises(window);
        self.mixer.editor_raises.activation = Some(window);
    }

    pub(crate) fn raise_editors_over(&mut self, window: u64) {
        self.queue_editor_raises(window);
        self.pump_editor_raises();
    }

    fn queue_editor_raises(&mut self, window: u64) {
        let stack = self.editor_stack();
        let mut ids: Vec<_> = self.mixer.editors.iter().map(|e| e.id).collect();
        ids.sort_by_key(|id| {
            self.mixer
                .editor_touches
                .get(id)
                .and_then(|(hwnd, _)| stack.iter().position(|h| h == hwnd))
                .unwrap_or(usize::MAX)
        });
        self.mixer.editor_raises.base = window;
        self.mixer.editor_raises.request(ids);
        #[cfg(windows)]
        {
            self.mixer.editor_raises.ara_pending = !self.ara.editor_windows.is_empty();
        }
    }

    pub(crate) fn finish_editor_open(&mut self) -> bool {
        let mut foreground = None;
        let touches = &self.mixer.editor_touches;
        let mut request = None;
        for editor in &mut self.mixer.editors {
            if let Some(loading) = editor.retired_loading
                && editor.foreground_from != 0
                && touches.contains_key(&editor.id)
            {
                let foreground = *foreground.get_or_insert_with(Self::editor_foreground);
                // GPUI closes native windows asynchronously. Let Windows return focus
                // from the loading window to the studio window that opened it.
                if loading != 0 && loading == foreground {
                    continue;
                }
                let source = std::mem::take(&mut editor.foreground_from);
                if source == foreground {
                    request = Some((editor.id, source));
                }
            }
        }
        if let Some(request) = request {
            self.mixer.editor_raises.foreground = Some(request);
            self.raise_editors_over(0);
            return true;
        }
        false
    }

    pub(crate) fn pump_editor_raises(&mut self) {
        #[cfg(windows)]
        if self.mixer.editor_raises.activation.is_some()
            && (!self.mixer.editor_raises.pending.is_empty()
                || self.mixer.editor_raises.ara_pending)
        {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
            };
            // The click's remaining input can revoke a host's foreground grant.
            // Let Windows finish the click and place the palette group first.
            let pressed = !cfg!(test)
                && [VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2]
                    .into_iter()
                    // SAFETY: only reads the current button state.
                    .any(|key| unsafe { GetAsyncKeyState(key.into()) < 0 });
            if !self
                .mixer
                .editor_raises
                .activation_ready(Self::editor_foreground(), pressed)
            {
                return;
            }
        }
        #[cfg(windows)]
        if std::mem::take(&mut self.mixer.editor_raises.ara_pending) {
            let stack = self.editor_stack();
            let base = self.editor_stack_base(self.mixer.editor_raises.base);
            for &hwnd in stack.iter().rev() {
                if self
                    .ara
                    .editor_windows
                    .values()
                    .any(|(window, _)| *window == hwnd)
                {
                    xirius_vst3::raise_independent_editor(hwnd, &stack, base);
                }
            }
        }
        let Some((id, ticket)) = self.mixer.editor_raises.next(std::time::Instant::now()) else {
            return;
        };
        let Some(mut bar) = self.mixer.editor_bars.get(&id).cloned() else {
            self.mixer.editor_raises.ack(id, ticket);
            return;
        };
        bar.stack = self.editor_stack();
        bar.raise_ticket = ticket;
        let mut foreground_from = 0;
        if let Some((editor, source)) = self.mixer.editor_raises.foreground
            && editor == id
        {
            self.mixer.editor_raises.foreground = None;
            if Self::editor_foreground() == source {
                foreground_from = source;
            }
        }
        let base = self.mixer.editor_raises.base;
        if base != 0 {
            bar.stack_base = base;
        }
        #[cfg(windows)]
        unsafe {
            // SAFETY: queries and a foreground grant; no call waits on a plugin thread.
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                AllowSetForegroundWindow, GetForegroundWindow, GetWindowThreadProcessId,
            };
            let foreground = GetForegroundWindow();
            let mut process = 0;
            GetWindowThreadProcessId(foreground, &mut process);
            if process == std::process::id() {
                if let Some((window, _)) = self.mixer.editor_touches.get(&id)
                    && GetWindowThreadProcessId(*window as _, &mut process) != 0
                {
                    AllowSetForegroundWindow(process);
                }
            }
            bar.stack_base = self.editor_stack_base(base);
        }
        if let Some(host) = self.transport.host() {
            host.plugin_editor_bar(id, bar);
            host.raise_plugin_editor(id, foreground_from);
        }
    }

    #[cfg(windows)]
    fn editor_stack_base(&self, requested: u64) -> u64 {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GW_HWNDPREV, GW_OWNER, GetForegroundWindow, GetWindow, IsWindowVisible,
        };
        let mut base = if requested == 0 {
            let foreground = Self::editor_foreground();
            if foreground != 0 {
                foreground
            } else {
                crate::park::own_handle(crate::screen::WINDOW_TITLE).unwrap_or(0) as u64
            }
        } else {
            requested
        };
        // SAFETY: only queries the desktop's window handles and ownership.
        unsafe {
            let foreground = GetForegroundWindow();
            let window = base as windows_sys::Win32::Foundation::HWND;
            let owner = GetWindow(window, GW_OWNER);
            let root = if owner.is_null() { window } else { owner };
            if !owner.is_null() && window == foreground {
                // The active palette is the ceiling for the external editors.
                base = root as u64;
            } else if !root.is_null() {
                // Clicking the main window raises its owned group as well.
                let mut above = GetWindow(window, GW_HWNDPREV);
                for _ in 0..4096 {
                    if above.is_null() {
                        break;
                    }
                    if IsWindowVisible(above) != 0 && GetWindow(above, GW_OWNER) == root {
                        base = above as u64;
                    }
                    above = GetWindow(above, GW_HWNDPREV);
                }
            }
        }
        base
    }

    /// Where a window of this process opens: over the studio's window and above it, or off
    /// every screen under tests.
    pub(crate) fn editor_placement(&self) -> Placement {
        let owner = crate::park::own_handle(crate::screen::WINDOW_TITLE);
        let place = if cfg!(test) {
            Placement {
                owner,
                ..Placement::offscreen()
            }
        } else {
            Placement::shown(owner)
        };
        Placement {
            caption: self.caption,
            ..place
        }
    }

    /// The windows' captions take the colours of the app's top bar in `theme`: at once for
    /// the windows that are open, and from the first frame for the ones that open later
    /// (UI-183).
    pub fn set_caption_theme(&mut self, theme: xirius_design::Theme) {
        use xirius_design::{Surface, Theme};
        let bar = Surface::S0.strong_text().resolve(theme);
        let caption = Caption {
            dark: theme == Theme::Dark,
            fill: bar.surface.to_rgb_u32(),
            text: bar.ink.to_rgb_u32(),
        };
        if self.caption == Some(caption) {
            return;
        }
        self.caption = Some(caption);
        for e in &self.mixer.editors {
            if let Some(host) = self.transport.host() {
                host.plugin_caption(e.id, caption);
            }
        }
        if let Some(host) = self.transport.host() {
            host.set_ara_editor_caption(caption);
        }
    }

    #[cfg(windows)]
    pub(crate) fn device_window_handle(&self, id: PluginId) -> Option<isize> {
        self.mixer
            .editor_hwnd(id)
            .or_else(|| crate::park::own_handle(&format!("Xirius · {id}")))
    }

    /// Opens `plugin`'s own window, on the instance that plays it. `at` keeps the corner of a
    /// window that opens again.
    pub(crate) fn open_vst3_window(
        &mut self,
        plugin: &Plugin,
        title: String,
        foreground_from: u64,
    ) -> Result<OpenEditor, String> {
        self.mixer.editor_bars.remove(&plugin.id);
        let _filter = xirius_engine::crash::guard_exception_filter();
        let name = plugin.name.as_str();
        if plugin.format != "vst3" {
            return Err(cannot(name, "は VST3 ではない"));
        }
        let catalog = self.plugin_catalog();
        if catalog.find(&plugin.uid).is_none() {
            return Err(cannot(name, "がこの PC に見つからない"));
        }
        let mut base = self.editor_placement();
        if let Some(area) = &mut base.area {
            if let Some(source) = xirius_vst3::EditorArea::from_window(foreground_from as isize) {
                area.main = source.main;
                area.work = source.work;
            }
            area.restore = self.mixer.editor_positions.get(&plugin.id).copied();
            #[cfg(windows)]
            if let Some(previous) = self
                .normal_view_before(plugin.id)
                .and_then(|id| self.device_window_handle(id))
                .and_then(xirius_vst3::EditorArea::from_window)
            {
                area.work = previous.work;
                area.restore = Some((previous.main[0], previous.main[1]));
                area.replacing = true;
                area.retiring = Some(previous.main_window);
            }
        }
        let project = self.auto_heard(&self.project());
        let bar = self.vst_bar(plugin);
        let dir = self.song.dir();
        let host = self.transport.start_host(&project, dir)?;
        let status = host
            .plugin_statuses()
            .into_iter()
            .find(|(id, ..)| *id == plugin.id)
            .map(|(.., s)| s);
        let Some(status) = status else {
            // The player has no instance of it: it could not be loaded.
            return Err(cannot(name, "を読み込めなかった"));
        };
        if status.crashed {
            return Err(cannot(name, "が止まっている"));
        }
        if status.error.is_some() {
            return Err(cannot(name, "を読み込めなかった"));
        }
        if let Some(area) = base.area {
            host.plugin_editor_area(plugin.id, area);
        }
        host.plugin_editor_bar(plugin.id, bar);
        host.plugin_editor(plugin.id, true, self.caption)
            .map_err(|e| format!("{name} の窓を開けません: {e}"))?;
        let title = host
            .plugins()
            .editor_window_title(plugin.id)
            .unwrap_or(title);
        Ok(OpenEditor {
            id: plugin.id,
            title,
            at: Where::Host { opened: false },
            foreground_from,
            retired_loading: None,
        })
    }

    /// Shuts a plugin's window.
    pub(crate) fn close_editor(&mut self, id: PluginId) {
        if self
            .mixer
            .editor_raises
            .foreground
            .is_some_and(|(editor, _)| editor == id)
        {
            self.mixer.editor_raises.foreground = None;
        }
        self.mixer.editor_bars.remove(&id);
        self.mixer.editor_touches.remove(&id);
        self.mixer
            .editor_pending
            .retain(|(other, _, _)| *other != id);
        let gone: Vec<OpenEditor> = self.mixer.take_editors(|e| e.id == id);
        let shut = !gone.is_empty();
        for _ in gone {
            if let Some(host) = self.transport.host_mut() {
                let _ = host.plugin_editor(id, false, None);
            }
        }
        if shut {
            // What was done in the window is in the song when it is gone (PLG-057).
            self.pull_plugins();
        }
    }

    /// Asks every plugin for its state and takes what differs from the song's into it, as one
    /// step of the history (PLG-057). A plugin that changes its patch in its own window does
    /// not always say so, and the song would keep the state it was opened with. For the
    /// moments the song is written or left, and when a window closes.
    pub(crate) fn pull_plugins(&mut self) {
        let Some(host) = self.transport.host_mut() else {
            return;
        };
        // A frame will collect the result. Save/open/quit use their explicit
        // StatePull preflight; this helper must never spin in a window callback.
        host.begin_pull_states();
    }

    /// Bring an existing window forward through the same stack queue as a newly opened editor.
    pub(crate) fn raise_editor(&mut self, id: PluginId, source: u64) {
        if let Some((_, time)) = self.mixer.editor_touches.get_mut(&id) {
            *time = editor_touch_time();
        }
        if source != 0 {
            self.mixer.editor_raises.foreground = Some((id, source));
        }
        self.raise_editors_over(0);
    }

    /// The window of `id` is open and shown.
    pub(crate) fn editor_shown(&self, id: PluginId) -> bool {
        self.mixer
            .editors
            .iter()
            .filter(|e| e.id == id)
            .any(|e| self.shown(e))
    }

    fn shown(&self, e: &OpenEditor) -> bool {
        #[cfg(test)]
        if let Some(host) = &self.mixer.fake_load {
            return host.loaded.contains(&e.id);
        }
        match &e.at {
            Where::Host { opened } => {
                let status = self.transport.host().and_then(|host| {
                    host.plugin_statuses()
                        .into_iter()
                        .find(|(id, ..)| *id == e.id)
                        .map(|(.., status)| status)
                });
                status
                    .is_some_and(|s| !s.crashed && s.error.is_none() && (!*opened || s.editor_open))
            }
        }
    }

    /// The host process has said the window of `id` is open: from then on, only its word
    /// decides whether the window is shown.
    pub(crate) fn host_said_editor_open(&self, id: PluginId) -> bool {
        self.mixer
            .editors
            .iter()
            .any(|e| e.id == id && matches!(e.at, Where::Host { opened: true, .. }))
    }

    /// Between frames too: forgets the windows the person closed, opens a window again on
    /// the instance of a new mix, and takes what was turned in the plugins' windows into the
    /// song. True when the song changed.
    pub fn poll_editors(&mut self) -> bool {
        let bar = self.poll_editor_bar();
        let keys = bar | self.poll_editor_keys();
        let mut failed = Vec::new();
        let mut opened_sizes = Vec::new();
        if let Some(host) = self.transport.host() {
            for editor in &mut self.mixer.editors {
                for event in host.take_editor_open_events(editor.id) {
                    match event {
                        EditorOpenEvent::Opening => {}
                        EditorOpenEvent::Opened { size, .. } => {
                            let Where::Host { opened } = &mut editor.at;
                            *opened = true;
                            opened_sizes.push((editor.id, size));
                        }
                        EditorOpenEvent::Failed { reason } => failed.push((editor.id, reason)),
                    }
                }
            }
        }
        let opened = !opened_sizes.is_empty();
        if !opened_sizes.is_empty() {
            for &(id, _) in &opened_sizes {
                if !failed.iter().any(|(failed, _)| *failed == id) {
                    self.finish_device_open(id);
                }
            }
            let project = self.project();
            let sizes: Vec<_> = opened_sizes
                .into_iter()
                .filter_map(|(id, size)| {
                    project.tracks.values().find_map(|t| {
                        t.inserts
                            .iter()
                            .find(|p| p.id == id)
                            .map(|p| (p.uid.clone(), size))
                    })
                })
                .collect();
            self.mixer.editor_sizes.extend(sizes);
        }
        let had_failed = !failed.is_empty();
        for (id, reason) in failed {
            self.close_editor(id);
            self.error = Some(reason);
        }
        let gone: Vec<PluginId> = self
            .mixer
            .editors
            .iter()
            .filter(|e| !self.shown(e))
            .map(|e| e.id)
            .collect();
        let had_gone = !gone.is_empty();
        for id in gone {
            self.close_editor(id);
        }
        let lifecycle = opened | had_failed | had_gone;
        // While the song is being left, answers to the close's own pulls go in at the cursor:
        // they are not the person's edits (UI-354).
        let leaving = self.leaving_song();
        let Some(host) = self.transport.host_mut() else {
            return keys | lifecycle;
        };
        let edits = match if leaving {
            self.song.collect_plugin_edits_in_place(host)
        } else {
            self.song.collect_plugin_edits(host)
        } {
            Ok(false) => false,
            Ok(true) => {
                self.after_edit();
                true
            }
            Err(e) => {
                self.error = Some(e);
                true
            }
        };
        keys | edits | lifecycle
    }

    pub(crate) fn poll_editor_bar(&mut self) -> bool {
        use xirius_vst3::EditorAction;
        let mut actions = std::mem::take(&mut self.mixer.editor_pending);
        let p = self.project();
        for e in &self.mixer.editors {
            let origin = self
                .transport
                .host()
                .and_then(|h| h.plugins().editor_origin(e.id));
            if let Some(origin) = origin {
                self.mixer.editor_positions.insert(e.id, origin);
            }
            let Some((track, plugin)) = p
                .tracks
                .values()
                .find_map(|t| t.inserts.iter().find(|p| p.id == e.id).map(|p| (t, p)))
            else {
                continue;
            };
            let bar = self.vst_bar(plugin);
            if self.mixer.editor_bars.get(&e.id) != Some(&bar) {
                if let Some(host) = self.transport.host() {
                    host.plugin_editor_bar(e.id, bar.clone());
                }
                self.mixer.editor_bars.insert(e.id, bar);
            }
            let events = self
                .transport
                .host()
                .map_or_else(Vec::new, |h| h.take_editor_actions(e.id));
            actions.extend(events.into_iter().map(|a| (e.id, track.id, a)));
        }
        let mut changed = false;
        let mut new_editor = false;
        for (id, track, action) in actions {
            if !self.mixer.is_open(id) {
                continue;
            }
            let needs_song = matches!(
                action,
                EditorAction::Power
                    | EditorAction::PresetSelect(_)
                    | EditorAction::PresetPick(_)
                    | EditorAction::PresetPrevious
                    | EditorAction::PresetNext
                    | EditorAction::Sidechain
            );
            if needs_song
                && (self.input_blocked() || (self.macro_run.is_some() && !self.macro_stepping))
            {
                self.mixer.editor_pending.push((id, track, action));
                continue;
            }
            changed = true;
            match action {
                EditorAction::EditorRaised(ticket) => self.mixer.editor_raises.ack(id, ticket),
                EditorAction::EditorTouched { window, time } => {
                    let entry = self.mixer.editor_touches.entry(id).or_default();
                    if time >= entry.1 {
                        new_editor |= entry.0 != window;
                        *entry = (window, time);
                    }
                }
                EditorAction::Power => {
                    // design-lint: allow(ui-blocking-io) PRJ-031: guarded dispatcher enqueues the model edit; the plugin GUI never waits for it.
                    self.run_command(
                        "plugin.bypass",
                        serde_json::json!({"ids": [id.to_string()]}),
                    );
                }
                EditorAction::Pin => {
                    if self.mixer.editor_pins.remove(&id) {
                        self.unpinned_device_view(id);
                    } else {
                        self.mixer.editor_pins.insert(id);
                    }
                }
                EditorAction::Channel => self.open_channel_beside(
                    track,
                    id,
                    self.mixer.editor_hwnd(id).unwrap_or_default() as u64,
                ),
                EditorAction::PresetOpen => {
                    self.file_request = Some(crate::studio::FileRequest::PresetOpen(id))
                }
                EditorAction::PresetSave => {
                    self.file_request = Some(crate::studio::FileRequest::PresetSave(id))
                }
                EditorAction::PresetPick(key) => {
                    self.inst_menu_pick(crate::instrument::menu::Menu::Preset(track, id), &key);
                    if key == crate::instrument::menu::SAVE_AS && self.popup.is_some() {
                        self.mixer.preset_save = Some(id);
                    }
                }
                EditorAction::PresetSelect(preset) => {
                    // design-lint: allow(ui-blocking-io) UI-337: the guarded dispatcher loads on the song worker; the plugin GUI only reports the selected name.
                    self.run_command(
                        "preset.load",
                        serde_json::json!({"track":track, "of":id, "preset":preset}),
                    );
                }
                EditorAction::PresetPrevious | EditorAction::PresetNext => {
                    self.step_device_preset(id, action == EditorAction::PresetNext);
                }
                EditorAction::Sidechain => {
                    if let Some(sc) = p
                        .tracks
                        .get(&track)
                        .and_then(|t| t.inserts.iter().find(|p| p.id == id))
                        .and_then(|p| p.sidechain.as_ref())
                    {
                        // design-lint: allow(ui-blocking-io) PRJ-031: guarded dispatcher queues the existing sidechain command.
                        self.run_command("plugin.sidechain.enable", serde_json::json!({"id": id.to_string(), "state": if sc.on { "off" } else { "on" }}));
                    }
                }
            }
        }
        // The first touch carries the HWND. An earlier Opened reply can arrive before it,
        // when the new editor still has no position in the last-used order.
        #[cfg(windows)]
        if new_editor {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowThreadProcessId,
            };
            let mut process = 0;
            // SAFETY: inspect the foreground process without sending it a window message.
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut process) };
            if process == std::process::id() {
                self.raise_editors_over(0);
                return changed;
            }
        }
        #[cfg(not(windows))]
        let _ = new_editor;
        self.pump_editor_raises();
        changed
    }
}

/// Only one host places a frame at a time. A hung host cannot hold up other windows.
#[derive(Default)]
pub(crate) struct RaiseQueue {
    base: u64,
    #[cfg(windows)]
    activation: Option<u64>,
    #[cfg(windows)]
    ara_pending: bool,
    foreground: Option<(PluginId, u64)>,
    pending: std::collections::VecDeque<PluginId>,
    flight: Option<(PluginId, u64, std::time::Instant)>,
    serial: u64,
}
impl RaiseQueue {
    fn request(&mut self, ids: Vec<PluginId>) {
        #[cfg(windows)]
        {
            self.activation = None;
        }
        self.pending = ids.into();
    }

    #[cfg(windows)]
    fn activation_ready(&mut self, foreground: u64, mouse_down: bool) -> bool {
        if mouse_down {
            return false;
        }
        if self.activation != Some(foreground) {
            self.pending.clear();
            self.ara_pending = false;
            self.activation = None;
            return false;
        }
        true
    }
    fn ack(&mut self, id: PluginId, ticket: u64) {
        if self
            .flight
            .is_some_and(|(current, serial, _)| current == id && serial == ticket)
        {
            self.flight = None;
        }
    }
    fn next(&mut self, now: std::time::Instant) -> Option<(PluginId, u64)> {
        if self.flight.is_some_and(|(_, _, sent)| {
            now.duration_since(sent) < std::time::Duration::from_millis(500)
        }) {
            return None;
        }
        self.flight = None;
        let id = self.pending.pop_front()?;
        self.serial = self.serial.wrapping_add(1);
        self.flight = Some((id, self.serial, now));
        Some((id, self.serial))
    }
}

#[cfg(test)]
mod activation_tests {
    use super::*;

    #[test]
    fn raising_waits_for_the_matching_host_and_skips_a_hung_one() {
        let now = std::time::Instant::now();
        let mut q = RaiseQueue::default();
        q.request(vec![PluginId(3), PluginId(1), PluginId(2)]);
        let (id, ticket) = q.next(now).unwrap();
        assert_eq!(id, PluginId(3));
        assert!(q.next(now).is_none());
        q.ack(PluginId(1), ticket);
        assert!(q.next(now).is_none());
        q.ack(id, ticket);
        assert_eq!(q.next(now).unwrap().0, PluginId(1));
        assert_eq!(
            q.next(now + std::time::Duration::from_secs(1)).unwrap().0,
            PluginId(2)
        );
        q.ack(id, ticket);
        assert!(q.next(now + std::time::Duration::from_secs(1)).is_none());
    }

    #[test]
    fn a_new_activation_replaces_pending_order_without_concurrent_moves() {
        let now = std::time::Instant::now();
        let mut q = RaiseQueue::default();
        q.request(vec![PluginId(1), PluginId(2)]);
        let (id, ticket) = q.next(now).unwrap();
        q.request(vec![PluginId(2), PluginId(1)]);
        assert!(q.next(now).is_none());
        q.ack(id, ticket);
        let (id, ticket) = q.next(now).unwrap();
        assert_eq!(id, PluginId(2));
        q.ack(id, ticket);
        assert_eq!(q.next(now).unwrap().0, PluginId(1));
    }

    #[cfg(windows)]
    #[test]
    fn a_palette_activation_waits_for_mouse_up_and_only_raises_for_the_active_palette() {
        let mut q = RaiseQueue::default();
        q.request(vec![PluginId(1)]);
        q.activation = Some(21);
        assert!(!q.activation_ready(21, true));
        assert!(!q.pending.is_empty());
        assert!(q.activation_ready(21, false));
        assert!(q.next(std::time::Instant::now()).is_some());

        q.request(vec![PluginId(2)]);
        q.activation = Some(21);
        assert!(!q.activation_ready(22, false));
        assert!(q.pending.is_empty());
    }
}
