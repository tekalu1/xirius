//! The spinner on a browser item while its plugin loads, and the turn of every spinner.
//!
//! A plugin is opened by the plugin host when the song plays it, so a heavy one can take a
//! while to answer. The item of the list the person used shows the spinner in the star's place
//! until the host reports the plugin has played, has failed, or has gone down.

use xirius_core::id::PluginId;
use xirius_core::store::now_ms;

use crate::studio::Studio;

/// How long a spinner is shown at most; a plugin that never reports must not spin for ever.
pub(crate) const GIVE_UP_MS: u64 = 8_000;
/// One turn of a spinner.
pub(crate) const TURN_MS: u64 = 900;

/// A plugin of the list that is being loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loading {
    pub name: String,
    pub id: PluginId,
    pub since: u64,
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
}
