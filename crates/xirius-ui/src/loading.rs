//! The spinner on a browser item while its plugin loads, and the turn of every spinner.
//!
//! A plugin is opened by the plugin host when the song plays it, so a heavy one can take a
//! while to answer. The item of the list the person used shows the spinner in the star's place
//! until the host reports the plugin has played, has failed, or has gone down.

use xirius_core::id::{PluginId, TrackId};
use xirius_core::store::now_ms;

use crate::studio::Studio;

/// How long a spinner is shown at most; a plugin that never reports must not spin for ever.
pub(crate) const GIVE_UP_MS: u64 = 8_000;
/// One turn of a spinner.
pub(crate) const TURN_MS: u64 = 900;
/// How long a window asked for with a plugin just put in waits for the plugin host. A heavy
/// plugin takes a while; one never reported is said as not loaded.
pub(crate) const OPEN_GIVE_UP_MS: u64 = 30_000;

/// A plugin of the list that is being loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loading {
    pub name: String,
    pub id: PluginId,
    pub since: u64,
}

/// A plugin just put in whose window opens once the plugin host has it (PLG-050).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenLater {
    pub track: TrackId,
    pub id: PluginId,
    pub since: u64,
}

/// The plugin host of a test: which plugins it has, and the windows asked for of it.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeLoad {
    pub loaded: Vec<PluginId>,
    pub asked: Vec<(TrackId, PluginId)>,
}

impl Studio {
    /// How far every spinner has turned now (a part of a turn); still when motion is reduced.
    pub(crate) fn spin(&self) -> f32 {
        if self.ai.motion.reduced {
            0.0
        } else {
            (now_ms() % TURN_MS) as f32 / TURN_MS as f32
        }
    }

    /// The item `name` of the list was used and its plugin `id` is on its way in. Built-in
    /// items load at once; with no player there is nothing to wait for.
    pub(crate) fn begin_loading(&mut self, name: &str, format: &str, id: PluginId) {
        if format == "builtin" || self.transport.host().is_none() {
            return;
        }
        self.browser.loading = Some(Loading {
            name: name.to_string(),
            id,
            since: now_ms(),
        });
    }

    /// Once a frame. True while a spinner is on screen, so frames keep coming.
    pub(crate) fn poll_loading(&mut self) -> bool {
        let busy = self.browser.loading.is_some()
            || self.browser.scanning
            || self.browser.retrying.is_some();
        if !busy {
            return false;
        }
        let now = now_ms();
        self.browser.spin = self.spin();
        let Some(l) = self.browser.loading.clone() else {
            return true;
        };
        let reported = self.transport.host().is_some_and(|h| {
            h.plugins().reports().iter().any(|r| {
                r.id == l.id
                    && (r.status.wet_blocks + r.status.dry_blocks > 0
                        || r.status.error.is_some()
                        || r.status.crashed)
            })
        });
        if reported || now.saturating_sub(l.since) > GIVE_UP_MS {
            self.browser.loading = None;
        }
        true
    }

    /// Opens the window of `id`, just put on track `t` (PLG-050). The plugin host of the
    /// window opens a plugin a while after the song has it, so a window asked for before is
    /// asked for once the host has it ([`Studio::poll_open_later`]); the screen does not wait.
    pub(crate) fn open_when_loaded(&mut self, t: TrackId, id: PluginId) {
        self.mixer.open_later = None;
        let built_in = self
            .project()
            .tracks
            .get(&t)
            .and_then(|x| x.inserts.iter().find(|p| p.id == id))
            .is_some_and(crate::instrument::window::is_built_in);
        if !built_in && self.loads_apart() && !self.has_loaded(id) {
            self.mixer.open_later = Some(OpenLater {
                track: t,
                id,
                since: now_ms(),
            });
            return;
        }
        self.open_plugin(t, id);
    }

    /// Whether plugins put in are opened by the window's plugin host a while after the song
    /// has them.
    fn loads_apart(&self) -> bool {
        #[cfg(test)]
        if self.mixer.fake_load.is_some() {
            return true;
        }
        self.transport.is_windowed() && self.transport.host().is_some()
    }

    /// Whether the plugin host has `id`, played, failed or gone down: its window can be asked
    /// for, or says why not.
    pub(crate) fn has_loaded(&self, id: PluginId) -> bool {
        #[cfg(test)]
        if let Some(fake) = &self.mixer.fake_load {
            return fake.loaded.contains(&id);
        }
        self.transport.host().is_some_and(|h| {
            h.plugins().local(id).is_some() || h.plugin_statuses().iter().any(|(at, ..)| *at == id)
        })
    }

    /// Once a frame: the window asked for with a plugin just put in opens, once, when the host
    /// has it. It does not when the plugin was taken out or another song is open; a plugin
    /// that failed is said as a window that cannot open. True while it waits, so frames keep
    /// coming.
    pub(crate) fn poll_open_later(&mut self) -> bool {
        let Some(w) = self.mixer.open_later else {
            return false;
        };
        let here = self
            .project()
            .tracks
            .get(&w.track)
            .is_some_and(|x| x.inserts.iter().any(|p| p.id == w.id));
        if !here {
            self.mixer.open_later = None;
            return false;
        }
        let waited = now_ms().saturating_sub(w.since) > OPEN_GIVE_UP_MS;
        if self.loads_apart() && !self.has_loaded(w.id) && !waited {
            return true;
        }
        self.mixer.open_later = None;
        #[cfg(test)]
        if let Some(fake) = &mut self.mixer.fake_load {
            fake.asked.push((w.track, w.id));
            return true;
        }
        self.open_plugin(w.track, w.id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_loading() -> Loading {
        Loading {
            name: "Surge XT".into(),
            id: PluginId(1),
            since: now_ms(),
        }
    }

    #[test]
    fn nothing_spins_without_a_reason_and_no_frames_are_asked_for() {
        let mut s = Studio::sample();
        assert!(!s.poll_loading());
        assert_eq!(s.browser.spin, 0.0);
    }

    #[test]
    fn a_scan_or_a_retry_turns_the_spinner_and_asks_for_frames() {
        let mut s = Studio::sample();
        s.ai.motion.reduced = false;
        s.browser.scanning = true;
        assert!(s.poll_loading());
        assert!((0.0..1.0).contains(&s.browser.spin));
        s.browser.scanning = false;
        s.browser.retrying = Some("C:/VST3/A.vst3".into());
        assert!(s.poll_loading());
        // Reduced motion: it still shows, it does not turn.
        s.ai.motion.reduced = true;
        assert!(s.poll_loading());
        assert_eq!(s.browser.spin, 0.0);
        s.browser.retrying = None;
        assert!(!s.poll_loading());
    }

    #[test]
    fn built_in_items_and_a_window_without_a_player_do_not_wait() {
        let mut s = Studio::sample();
        s.begin_loading("Sampler", "builtin", PluginId(1));
        assert_eq!(s.browser.loading, None);
        // The headless window has no player: nothing will ever report, so nothing is shown.
        s.begin_loading("Surge XT", "vst3", PluginId(1));
        assert_eq!(s.browser.loading, None);
    }

    #[test]
    fn a_spinner_that_is_never_answered_ends_by_itself() {
        let mut s = Studio::sample();
        let mut l = a_loading();
        l.since = now_ms().saturating_sub(GIVE_UP_MS + 1);
        s.browser.loading = Some(l);
        assert!(s.poll_loading());
        assert_eq!(s.browser.loading, None);
    }

    use crate::channel::flow::Chain;
    use crate::instrument::Pick;
    use serde_json::json;
    use xirius_core::rack::RackNode;

    fn vst3() -> Pick {
        Pick {
            name: "Test Gain".into(),
            format: "vst3".into(),
            uid: "test-gain".into(),
        }
    }

    /// The sample song in the window's way: a plugin put in is opened by the plugin host a
    /// while after the song has it, and the windows asked for are counted, not opened.
    fn apart() -> Studio {
        let mut s = Studio::sample();
        s.instruments.open.clear();
        s.mixer.fake_load = Some(FakeLoad::default());
        s
    }

    fn track(s: &Studio, name: &str) -> TrackId {
        s.project()
            .tracks
            .values()
            .find(|t| t.name == name)
            .unwrap()
            .id
    }

    fn ids(s: &Studio, t: TrackId) -> Vec<PluginId> {
        s.project().tracks[&t]
            .inserts
            .iter()
            .map(|p| p.id)
            .collect()
    }

    /// Puts the test plugin in at the end of `t`'s chain; its id.
    fn put_in(s: &mut Studio, t: TrackId) -> PluginId {
        let before = ids(s, t);
        assert!(s.add_effect(t, before.len(), &vst3()));
        new_one(s, t, &before)
    }

    fn new_one(s: &Studio, t: TrackId, before: &[PluginId]) -> PluginId {
        ids(s, t)
            .into_iter()
            .find(|id| !before.contains(id))
            .unwrap()
    }

    fn asked(s: &Studio) -> Vec<(TrackId, PluginId)> {
        s.mixer.fake_load.as_ref().unwrap().asked.clone()
    }

    fn loaded(s: &mut Studio, id: PluginId) {
        s.mixer.fake_load.as_mut().unwrap().loaded.push(id);
    }

    /// Frames while the host has not loaded `id`: nothing is asked for, and frames keep coming.
    fn frames_loading(s: &mut Studio, id: PluginId) {
        for _ in 0..5 {
            assert!(s.poll_open_later(), "frames keep coming while it loads");
        }
        assert!(asked(s).is_empty());
        assert!(!s.mixer.is_open(id));
        assert_eq!(s.error, None, "no 「読み込めなかった」 while it loads");
    }

    /// Frames once the host has `id`: its window is asked for once, on track `t`.
    fn frames_loaded(s: &mut Studio, t: TrackId, id: PluginId) {
        loaded(s, id);
        assert!(s.poll_open_later());
        for _ in 0..5 {
            assert!(!s.poll_open_later(), "no frames once it is asked for");
        }
        assert_eq!(asked(s), vec![(t, id)]);
        assert_eq!(s.mixer.open_later, None);
    }

    /// PLG-050: the window of a plugin just put in is asked for once the plugin host has it,
    /// exactly once, and not before.
    #[test]
    fn a_window_asked_with_a_plugin_put_in_opens_once_when_it_has_loaded() {
        let mut s = apart();
        let t = track(&s, "Vocal");
        let id = put_in(&mut s, t);
        assert_eq!(s.mixer.open_later.map(|w| (w.track, w.id)), Some((t, id)));
        frames_loading(&mut s, id);
        // Its row pressed while it loads: the window still opens once loaded.
        assert!(s.open_plugin(t, id));
        assert_eq!(s.error, None);
        frames_loading(&mut s, id);
        frames_loaded(&mut s, t, id);
    }

    /// A plugin the host failed to open, or that took the host down, is loaded as far as its
    /// window goes: the window is asked for once and says why it cannot open. One never
    /// reported is asked for once the wait is over, and says it was not loaded.
    #[test]
    fn a_plugin_that_failed_or_never_answers_is_asked_for_once() {
        let mut s = apart();
        let t = track(&s, "Vocal");
        let id = put_in(&mut s, t);
        frames_loading(&mut s, id);
        frames_loaded(&mut s, t, id);

        let mut s = apart();
        let id = put_in(&mut s, t);
        frames_loading(&mut s, id);
        if let Some(w) = &mut s.mixer.open_later {
            w.since = now_ms().saturating_sub(OPEN_GIVE_UP_MS + 1);
        }
        assert!(s.poll_open_later());
        assert!(!s.poll_open_later());
        assert_eq!(asked(&s), vec![(t, id)]);
    }

    /// The same in a splitter's branch and in a layer of a multi-instrument (MIX-042).
    #[test]
    fn a_plugin_put_in_a_branch_or_a_layer_opens_the_same() {
        let mut s = apart();
        let t = track(&s, "Vocal");
        assert!(s.run_command("rack.split.add", json!({ "track": t })));
        let branch = match &s.project().tracks[&t].rack.as_ref().unwrap().fx[..] {
            [.., RackNode::Split(x)] => x.branches[0].id,
            other => panic!("{other:?}"),
        };
        let before = ids(&s, t);
        assert!(s.add_effect_into(t, Chain::Branch(branch), 0, &vst3()));
        let id = new_one(&s, t, &before);
        frames_loading(&mut s, id);
        frames_loaded(&mut s, t, id);

        let mut s = apart();
        let k = track(&s, "Keys");
        for name in ["Piano", "Strings"] {
            assert!(s.run_command("rack.layer.add", json!({ "track": k, "name": name })));
        }
        let layer = crate::channel::keys::layers(&s.project().tracks[&k])[1].id;
        let before = ids(&s, k);
        assert!(s.add_effect_into(k, Chain::Layer(layer), 0, &vst3()));
        let id = new_one(&s, k, &before);
        frames_loading(&mut s, id);
        frames_loaded(&mut s, k, id);
    }

    /// The window is not asked for when its plugin was taken out, another song is open,
    /// another plugin was put in after it, or another window was opened, while it loaded.
    #[test]
    fn a_window_waiting_for_its_plugin_is_dropped_with_it() {
        let mut s = apart();
        let t = track(&s, "Vocal");
        let id = put_in(&mut s, t);
        frames_loading(&mut s, id);
        assert!(s.run_command("plugin.remove", json!({ "ids": [id.to_string()] })));
        loaded(&mut s, id);
        assert!(!s.poll_open_later());
        assert_eq!(s.mixer.open_later, None);
        assert!(asked(&s).is_empty(), "taken out");

        let id = put_in(&mut s, t);
        frames_loading(&mut s, id);
        s.new_song_sync();
        loaded(&mut s, id);
        assert!(!s.poll_open_later());
        assert!(asked(&s).is_empty(), "another song");

        let mut s = apart();
        let t = track(&s, "Vocal");
        let first = put_in(&mut s, t);
        let last = put_in(&mut s, t);
        loaded(&mut s, first);
        frames_loading(&mut s, last);
        frames_loaded(&mut s, t, last);

        let mut s = apart();
        let t = track(&s, "Vocal");
        assert!(s.run_command("plugin.add", json!({ "track": t, "name": "EQ" })));
        let eq = *ids(&s, t).last().unwrap();
        let id = put_in(&mut s, t);
        frames_loading(&mut s, id);
        assert!(s.open_plugin(t, eq));
        loaded(&mut s, id);
        assert!(!s.poll_open_later());
        assert!(asked(&s).is_empty(), "another window");
    }
}
