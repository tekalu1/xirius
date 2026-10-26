//! A VST3 plugin's own window (UI-040, PLG-027).
//!
//! The window shows the instance that plays, so what is turned in it is heard at once and,
//! once let go, becomes one step of the history ([`Studio::poll_editors`]):
//!
//! - a plugin that runs in the app: its instance in the player ([`PluginLoader::local`]),
//!   whose window this process opens, owned by the studio's window (it stays above it, goes
//!   with it, and never takes the focus when it opens);
//! - a plugin that runs in a plugin host process (shared or its own): the host opens the
//!   window of its instance, above the other windows.
//!
//! The player starts (playing nothing) when a window needs it. With no audio output there is
//! no player, and the window shows an instance of its own, whose changes are neither heard
//! nor kept. Under tests, and while [`xirius_vst3::OFFSCREEN_ENV`] is set, windows open off
//! every screen.
//!
//! [`PluginLoader::local`]: xirius_session::plugins::PluginLoader::local

use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_core::id::PluginId;
use xirius_core::model::Plugin;
use xirius_plugins::{decode_state, uid_hex};
use xirius_vst3::{Caption, Editor, Placement};

use crate::studio::Studio;

/// How long a window the plugin host was asked to open may take to say it is open.
const HOST_OPEN_WAIT: Duration = Duration::from_secs(3);

/// Where an open window's instance is.
pub(crate) enum Where {
    /// The player's instance in this process; `generation` tells a new mix's instance apart.
    Local { editor: Editor, generation: u64 },
    /// The plugin host process's instance; the host has the window. `opened` says the host has
    /// said the window is open once, so it is no longer taken as open while it does not answer.
    Host { asked: Instant, opened: bool },
    /// An instance of its own, made because nothing plays.
    Apart {
        // The window goes before the instance that owns its view.
        editor: Editor,
        _plugin: xirius_vst3::Plugin,
    },
}

/// A plugin's own window.
pub(crate) struct OpenEditor {
    pub id: PluginId,
    pub title: String,
    pub at: Where,
}

impl OpenEditor {
    pub fn param_titles(&self) -> Vec<(u32, String)> {
        match &self.at {
            Where::Apart { _plugin, .. } => _plugin
                .params()
                .into_iter()
                .map(|p| (p.id, p.title))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The window, when this process has it.
    pub fn editor(&self) -> Option<&Editor> {
        match &self.at {
            Where::Local { editor, .. } | Where::Apart { editor, .. } => Some(editor),
            Where::Host { .. } => None,
        }
    }

    pub fn editor_mut(&mut self) -> Option<&mut Editor> {
        match &mut self.at {
            Where::Local { editor, .. } | Where::Apart { editor, .. } => Some(editor),
            Where::Host { .. } => None,
        }
    }

    /// The instance plays: what is turned in the window is heard.
    pub fn heard(&self) -> bool {
        !matches!(self.at, Where::Apart { .. })
    }
}

/// A window to open again: the plugin, its title, and the corner it had, if this process had it.
type Reopen = (PluginId, String, Option<(i32, i32)>);

/// Why a plugin's window does not open, in the words the screen shows (ARC-027).
fn cannot(name: &str, why: &str) -> String {
    format!("{name} {why}ため、窓を開けません")
}

impl Studio {
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
            match (&e.at, e.editor()) {
                (_, Some(editor)) => editor.set_caption(caption),
                (Where::Host { .. }, None) => {
                    if let Some(host) = self.transport.host() {
                        host.plugin_caption(e.id, caption);
                    }
                }
                _ => {}
            }
        }
    }

    /// Opens `plugin`'s own window, on the instance that plays it. `at` keeps the corner of a
    /// window that opens again.
    pub(crate) fn open_vst3_window(
        &mut self,
        plugin: &Plugin,
        title: String,
        at: Option<(i32, i32)>,
    ) -> Result<OpenEditor, String> {
        let _filter = xirius_engine::crash::guard_exception_filter();
        let name = plugin.name.as_str();
        if plugin.format != "vst3" {
            return Err(cannot(name, "は VST3 ではない"));
        }
        let catalog = self.plugin_catalog();
        if catalog.find(&plugin.uid).is_none() {
            return Err(cannot(name, "がこの PC に見つからない"));
        }
        let base = self.editor_placement();
        let place = Placement {
            at: at.or(base.at),
            ..base
        };
        let fail = |e: xirius_vst3::Error| format!("{name} の窓を開けません: {e}");
        let project = self.auto_heard(&self.project());
        let dir = self.song.dir();
        let windowed = self.transport.is_windowed();
        let host = match self.transport.start_host(&project, dir) {
            Ok(host) => host,
            Err(error) if windowed => return Err(error),
            Err(_) => return open_apart(&catalog, plugin, title, place),
        };
        if let Some((instance, generation)) = host.plugins().local(plugin.id) {
            if windowed {
                return Err(
                    "専用所有スレッドのプラグインが準備できていません。音を準備し直してください"
                        .into(),
                );
            }
            let editor = instance.open_editor_at(&title, place).map_err(fail)?;
            return Ok(OpenEditor {
                id: plugin.id,
                title,
                at: Where::Local { editor, generation },
            });
        }
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
        host.plugin_editor(plugin.id, true, self.caption)
            .map_err(|e| format!("{name} の窓を開けません: {e}"))?;
        Ok(OpenEditor {
            id: plugin.id,
            title,
            at: Where::Host {
                asked: Instant::now(),
                opened: false,
            },
        })
    }

    /// Shuts a plugin's window.
    pub(crate) fn close_editor(&mut self, id: PluginId) {
        let gone: Vec<OpenEditor> = self.mixer.take_editors(|e| e.id == id);
        let shut = !gone.is_empty();
        for e in gone {
            if let Where::Host { .. } = e.at
                && let Some(host) = self.transport.host_mut()
            {
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

    /// Brings the open window of `id` above the other windows.
    pub(crate) fn raise_editor(&mut self, id: PluginId) {
        let mut in_host = false;
        for e in self.mixer.editors.iter().filter(|e| e.id == id) {
            match e.editor() {
                Some(editor) => editor.raise(),
                None => in_host = true,
            }
        }
        if in_host && let Some(host) = self.transport.host_mut() {
            let _ = host.plugin_editor(id, true, self.caption);
        }
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
        match &e.at {
            Where::Local { editor, .. } | Where::Apart { editor, .. } => editor.is_visible(),
            Where::Host { asked, opened } => {
                // Before the host says anything, the window counts as open for a while (it may
                // take time to make). Once it has said it is open, only the host's word decides:
                // a window it then says is shut is shut at once (its notice is not waited out).
                self.host_editor_open(e.id) || (!*opened && asked.elapsed() < HOST_OPEN_WAIT)
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

    /// The window of `id` is open in its host process, as the host last said.
    fn host_editor_open(&self, id: PluginId) -> bool {
        self.transport.host().is_some_and(|h| {
            h.plugin_statuses()
                .iter()
                .any(|(other, _, s)| *other == id && s.editor_open)
        })
    }

    /// Between frames too: forgets the windows the person closed, opens a window again on
    /// the instance of a new mix, and takes what was turned in the plugins' windows into the
    /// song. True when the song changed.
    pub fn poll_editors(&mut self) -> bool {
        let keys = self.poll_editor_keys();
        // Remember the host's word once it opens a window, so a later close is not waited out.
        let open_in_host: Vec<PluginId> = self
            .transport
            .host()
            .map(|h| {
                h.plugin_statuses()
                    .into_iter()
                    .filter(|(_, _, s)| s.editor_open)
                    .map(|(id, ..)| id)
                    .collect()
            })
            .unwrap_or_default();
        for e in &mut self.mixer.editors {
            if let Where::Host { opened, .. } = &mut e.at
                && !*opened
                && open_in_host.contains(&e.id)
            {
                *opened = true;
            }
        }
        let gone: Vec<PluginId> = self
            .mixer
            .editors
            .iter()
            .filter(|e| !self.shown(e) && !self.moved_here(e) && !self.moving_here(e))
            .map(|e| e.id)
            .collect();
        for id in gone {
            self.close_editor(id);
        }
        self.follow_mix();
        let Some(host) = self.transport.host_mut() else {
            return keys;
        };
        let edits = match self.song.collect_plugin_edits(host) {
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
        keys | edits
    }

    /// The window is the plugin host's, and the new mix plays the plugin in this process with
    /// the window still open (ENG-071): the host shut it, and it opens here.
    fn moved_here(&self, e: &OpenEditor) -> bool {
        matches!(e.at, Where::Host { .. })
            && self
                .transport
                .host()
                .is_some_and(|h| h.plugins().window_moved_in(e.id))
    }

    /// As [`Self::moved_here`], while the host has not let go of the instance that played: the
    /// window is kept until it is known whether the person shut it there as the mix changed.
    fn moving_here(&self, e: &OpenEditor) -> bool {
        matches!(e.at, Where::Host { .. })
            && self
                .transport
                .host()
                .is_some_and(|h| h.plugins().window_moving_in(e.id))
    }

    /// A new mix made new instances of the plugins in the app, or moved a plugin between the
    /// app and a plugin host process (ENG-071): their windows open again on the new ones,
    /// where they were. Without a player they shut.
    fn follow_mix(&mut self) {
        let stale: Vec<Reopen> = self
            .mixer
            .editors
            .iter()
            .filter_map(|e| {
                if self.moved_here(e) {
                    return Some((e.id, e.title.clone(), None));
                }
                let Where::Local { editor, generation } = &e.at else {
                    return None;
                };
                let now = self
                    .transport
                    .host()
                    .and_then(|h| h.plugins().local(e.id))
                    .map(|(_, g)| g);
                (now != Some(*generation)).then(|| (e.id, e.title.clone(), Some(editor.origin())))
            })
            .collect();
        for (id, title, at) in stale {
            self.mixer.take_editors(|e| e.id == id);
            if !self.transport.is_live() {
                continue;
            }
            let p = self.project();
            let Some(plugin) = p
                .tracks
                .values()
                .flat_map(|t| &t.inserts)
                .find(|x| x.id == id)
            else {
                continue;
            };
            match self.open_vst3_window(plugin, title, at) {
                Ok(e) => self.mixer.editors.push(e),
                Err(e) => self.error = Some(e),
            }
        }
    }
}

/// Makes an instance of the plugin of its own, with the song's state, and opens its window.
fn open_apart(
    catalog: &xirius_plugins::Catalog,
    plugin: &Plugin,
    title: String,
    place: Placement,
) -> Result<OpenEditor, String> {
    let name = plugin.name.as_str();
    let fail = |e: xirius_vst3::Error| format!("{name} の窓を開けません: {e}");
    let entry = catalog
        .find(&plugin.uid)
        .ok_or_else(|| cannot(name, "がこの PC に見つからない"))?;
    let module = xirius_vst3::Module::load(&entry.path).map_err(fail)?;
    let class = module
        .classes()
        .into_iter()
        .find(|c| uid_hex(&c.cid).eq_ignore_ascii_case(&plugin.uid))
        .ok_or_else(|| cannot(name, "がこの PC に見つからない"))?;
    let instance = xirius_vst3::Plugin::new(Arc::new(module), &class).map_err(fail)?;
    if let Some(state) = &plugin.state
        && let Ok(state) = decode_state(state)
    {
        // A state the plugin refuses leaves it at its defaults; the window still opens.
        let _ = instance.set_state(&state);
    }
    let editor = instance.open_editor_at(&title, place).map_err(fail)?;
    Ok(OpenEditor {
        id: plugin.id,
        title,
        at: Where::Apart {
            editor,
            _plugin: instance,
        },
    })
}
