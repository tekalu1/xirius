//! UI-055: the OS asks for less motion ("アニメーション効果" off on Windows). The setting is read
//! while the app runs, not once at the start: a change shows at the next frame it is drawn.
//! Every time-based change in the UI goes through [`xirius_design::Motion`] (panels and toggles
//! through `ViewMotion`, the AI's marks and glides through `ai`), so one flag makes them all 0 ms
//! and leaves the state itself (colour, shape, text) as it was.

use std::time::{Duration, Instant};

/// How often the OS is asked while frames are drawn.
const EVERY: Duration = Duration::from_secs(1);

/// Whether the OS asks for less motion.
#[cfg(windows)]
pub fn os_reduced() -> bool {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, out: *mut i32, ini: u32) -> i32;
    }
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    let mut on = 1;
    // SAFETY: the action writes one BOOL to `on`, which lives across the call.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut on, 0) };
    ok != 0 && on == 0
}

/// macOS "視差効果を減らす" (Accessibility → Display): the user default
/// `com.apple.universalaccess` / `reduceMotion`. Reading it on a real Mac is still to do
/// (UI-055's `remaining`); on other OSes nothing is read.
#[cfg(target_os = "macos")]
pub fn os_reduced() -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    let key = CFString::new("reduceMotion");
    let domain = CFString::new("com.apple.universalaccess");
    let mut exists: core_foundation_sys::base::Boolean = 0;
    // SAFETY: both strings live across the call; `exists` is written once by it.
    let on = unsafe {
        core_foundation_sys::preferences::CFPreferencesGetAppBooleanValue(
            key.as_concrete_TypeRef(),
            domain.as_concrete_TypeRef(),
            &mut exists,
        )
    };
    exists != 0 && on != 0
}

/// Linux has no one OS-wide setting for this; nothing is read.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn os_reduced() -> bool {
    false
}

/// Asks the OS at most once a second.
#[derive(Debug, Default)]
pub struct MotionWatch {
    last: Option<Instant>,
}

impl MotionWatch {
    /// The setting now, when it is time to ask; `None` while the last answer is fresh.
    pub fn poll(&mut self, now: Instant, read: impl FnOnce() -> bool) -> Option<bool> {
        if self.last.is_some_and(|t| now.duration_since(t) < EVERY) {
            return None;
        }
        self.last = Some(now);
        Some(read())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_motion::ViewMotion;
    use xirius_design::{Duration as Length, Motion};

    #[test]
    fn the_os_is_asked_once_a_second_and_a_change_is_seen() {
        let mut watch = MotionWatch::default();
        let t0 = Instant::now();
        assert_eq!(watch.poll(t0, || false), Some(false));
        assert_eq!(watch.poll(t0 + Duration::from_millis(900), || true), None);
        assert_eq!(
            watch.poll(t0 + Duration::from_millis(1000), || true),
            Some(true)
        );
        assert_eq!(watch.poll(t0 + Duration::from_millis(1500), || false), None);
        assert_eq!(
            watch.poll(t0 + Duration::from_millis(2100), || false),
            Some(false)
        );
    }

    /// With the setting on, every length is 0 and a change lands at once; the state it lands
    /// on (the toggle's value, the panel's rect) is the same as with motion.
    #[test]
    fn reduced_motion_lands_at_once_on_the_same_state() {
        let t0 = Instant::now();
        for length in Length::ALL {
            for reduced in [false, true] {
                let motion = Motion::new(reduced);
                let v = ViewMotion::default();
                v.enabled.set(true);
                v.value("on", 0.0, length, motion, t0);
                let at_once = v.value("on", 1.0, length, motion, t0);
                assert_eq!(at_once == 1.0, reduced, "{length:?} reduced={reduced}");
                assert_eq!(v.moving(), !reduced, "{length:?} reduced={reduced}");
                let end = t0 + Duration::from_millis(u64::from(length.ms()));
                assert_eq!(v.value("on", 1.0, length, motion, end), 1.0);
            }
        }
    }

    #[test]
    fn the_panels_do_not_slide_under_reduced_motion() {
        let v = ViewMotion::default();
        v.enabled.set(true);
        let panels = |browser| crate::studio::Panels {
            drawer: true,
            arrange: true,
            tempo: true,
            browser,
            mixer: false,
            folded: false,
            record: false,
        };
        let sized = |browser| {
            crate::studio::Geometry::sized_to(
                1280.0,
                760.0,
                panels(browser),
                xirius_design::Layout::TRACK_HEAD_W,
                None,
                xirius_design::Layout::BROWSER_W,
            )
        };
        let reduced = Motion::new(true);
        let _ = v.geometry(sized(false), reduced);
        let open = sized(true);
        assert_eq!(v.geometry(open, reduced), open);
        assert!(!v.moving());
        // With motion on, the same change slides.
        let w = ViewMotion::default();
        w.enabled.set(true);
        let _ = w.geometry(sized(false), Motion::default());
        assert_ne!(w.geometry(open, Motion::default()), open);
        assert!(w.moving());
    }

    /// The AI's marks hold still, and its glow stays lit: the state is there, the motion is not.
    #[test]
    fn the_ais_marks_hold_still_but_keep_their_state() {
        let mut s = crate::studio::Studio::sample();
        s.ai.clock = Some(1_000_000);
        s.ai.motion = Motion::new(true);
        assert_eq!(s.ai.flicker(0), (1.0, 1.0));
        assert_eq!(s.ai.flicker(300), (1.0, 1.0));
        assert_eq!(s.ai.motion.ai_loop_ms(), 0);
    }
}
