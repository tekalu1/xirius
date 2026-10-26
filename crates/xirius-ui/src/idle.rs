//! What the studio keeps asking while no frame is drawn (PLG-053).
//!
//! A minimized window draws nothing, so [`Studio::tick`] does not run. The screen asks
//! [`Studio::idle`] on a timer of its own instead, every [`crate::health::POLL`]: a plugin
//! that goes down while the window is away shows the moment it comes back, and the taskbar
//! button blinks once.

use crate::studio::Studio;

/// What one poll between frames found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Idle {
    /// Something on screen changed: draw again.
    pub changed: bool,
    /// A plugin went down since the last poll: ask for the person's attention.
    pub went_down: bool,
}

impl Studio {
    /// Asks the plugin host how its plugins are, as a frame would, takes in what was turned in
    /// the plugins' own windows, lets go of a notice said long enough, takes the device's channels and this PC's MIDI ports read meanwhile, and opens again a sound device that could not be had.
    pub fn idle(&mut self) -> Idle {
        let prefs = self.poll_browser_prefs_close() | self.poll_recent_close();
        let opened = self.poll_opening();
        if self.opening.is_some() {
            return Idle {
                changed: true,
                went_down: false,
            };
        }
        let saved = self.poll_saving();
        if self.saving.is_some() {
            return Idle {
                changed: true,
                went_down: false,
            };
        }
        if self.sound_preparing() {
            // Preparation owns the song. Poll its result/cleanup even when the
            // window is minimized, without entering unrelated monitor/file I/O.
            self.poll_preparing();
            return Idle {
                changed: true,
                went_down: false,
            };
        }
        let changed = self.poll_recent_keep()
            | self.poll_plugins()
            | self.poll_perf()
            | self.poll_editors()
            | self.expire_notice()
            | self.mixer.flash_expire(xirius_core::store::now_ms())
            | self.poll_input_channels()
            | self.poll_midi_ports()
            | self.poll_live_midi()
            | crate::wave::poll()
            | {
                let listed = crate::browser_files::poll();
                listed && self.browser.open && self.browser.tab == crate::browser::RailTab::Files
            }
            | self.poll_browser_menu()
            | self.poll_browser_import()
            | self.follow_browser()
            | self.follow_audition()
            | self.poll_export()
            | self.poll_job()
            | self.poll_processed()
            | self.expire_toast()
            | self.expire_gain_tip()
            | self.settings_reload()
            | self.settings_tick()
            | self.keep_beat(xirius_core::store::now_ms())
            | self.poll_device()
            | self.poll_dawproject()
            | self.poll_tidy();
        Idle {
            changed: changed | saved | opened | prefs,
            went_down: self.take_went_down(),
        }
    }

    /// True once after a plugin went down, whether a frame or [`Studio::idle`] saw it.
    pub fn take_went_down(&mut self) -> bool {
        std::mem::take(&mut self.health.went_down)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use xirius_core::model::TrackKind;

    use super::*;
    use crate::health::Report;

    fn with_effect() -> (Studio, xirius_core::id::TrackId, xirius_core::id::PluginId) {
        let mut s = Studio::sample();
        let t = s
            .project()
            .tracks
            .values()
            .find(|x| x.kind == TrackKind::Audio)
            .expect("an audio track")
            .id;
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": "Echo", "kind": "effect",
                    "format": "vst3", "uid": "00000000000000000000000000000001" }),
        ));
        let id = s.project().tracks[&t].inserts.last().unwrap().id;
        (s, t, id)
    }

    fn down(s: &mut Studio, id: xirius_core::id::PluginId, crashed: bool) {
        s.health.fake = Some(vec![Report {
            id,
            crashed,
            suggest: false,
        }]);
    }

    #[test]
    fn a_plugin_that_goes_down_without_a_frame_is_seen_by_the_idle_poll() {
        let (mut s, t, id) = with_effect();
        assert_eq!(s.idle(), Idle::default());
        down(&mut s, id, true);
        // No tick: the window draws nothing.
        std::thread::sleep(crate::health::POLL);
        assert_eq!(
            s.idle(),
            Idle {
                changed: true,
                went_down: true
            }
        );
        assert_eq!(s.crashed_on(t), Some(id));
        // Once only.
        std::thread::sleep(crate::health::POLL);
        assert_eq!(s.idle(), Idle::default());
    }

    #[test]
    fn a_crash_a_frame_saw_first_still_asks_for_attention_once() {
        let (mut s, _, id) = with_effect();
        down(&mut s, id, true);
        assert!(s.poll_plugins_now(), "a frame saw it");
        let idle = s.idle();
        assert!(idle.went_down && !idle.changed, "{idle:?}");
        assert!(!s.take_went_down());
    }

    #[test]
    fn a_restart_is_a_change_but_asks_for_nothing() {
        let (mut s, t, id) = with_effect();
        down(&mut s, id, true);
        assert!(s.poll_plugins_now());
        s.take_went_down();
        down(&mut s, id, false);
        std::thread::sleep(crate::health::POLL);
        assert_eq!(
            s.idle(),
            Idle {
                changed: true,
                went_down: false
            }
        );
        assert_eq!(s.crashed_on(t), None);
    }
}
