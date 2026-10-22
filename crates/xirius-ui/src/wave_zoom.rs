//! The vertical zoom of the waveforms (UI-310): ×1, ×2 or ×4, only how tall they are drawn; the
//! sound is not changed. The button after the zoom buttons at the head of the column goes round
//! the three, and Alt + wheel over the lanes steps up and down. Like the time zoom, it is the
//! screen's and the song does not keep it.

use crate::studio::Studio;

/// The vertical zooms, as the mock's ×1・×2・×4.
pub const STEPS: [f32; 3] = [1.0, 2.0, 4.0];

fn index(zoom: f32) -> usize {
    STEPS
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - zoom).abs().total_cmp(&(b.1 - zoom).abs()))
        .map_or(0, |(i, _)| i)
}

impl Studio {
    /// The step nearest `zoom`.
    pub fn set_wave_zoom(&mut self, zoom: f32) {
        self.wave_zoom = STEPS[index(zoom)];
    }

    /// `by` steps taller (or lower, below 0), stopping at ×1 and ×4; true when it moved.
    pub fn step_wave_zoom(&mut self, by: i32) -> bool {
        let i = (index(self.wave_zoom) as i32 + by).clamp(0, STEPS.len() as i32 - 1);
        let before = self.wave_zoom;
        self.wave_zoom = STEPS[i as usize];
        self.wave_zoom != before
    }

    /// The button: the next step, and ×1 again after ×4.
    pub fn cycle_wave_zoom(&mut self) {
        self.wave_zoom = STEPS[(index(self.wave_zoom) + 1) % STEPS.len()];
    }

    /// Alt + wheel turned `notches` (away from you is up): a step a whole notch. True when the
    /// turn was taken, even short of a notch.
    pub(crate) fn wave_zoom_wheel(&mut self, notches: f32) -> bool {
        if self.wave_zoom_turn * notches < 0.0 {
            self.wave_zoom_turn = 0.0;
        }
        self.wave_zoom_turn += notches;
        let whole = self.wave_zoom_turn.trunc();
        if whole != 0.0 {
            self.wave_zoom_turn -= whole;
            self.step_wave_zoom(whole as i32);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::screen::ZOOM_NOTCH_PX;
    use crate::studio::{Geometry, Hit, Studio};
    use crate::tools::Mods;

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 900.0);
        s.resized(&g);
        (s, g)
    }

    fn alt() -> Mods {
        Mods {
            ctrl: false,
            shift: false,
            alt: true,
        }
    }

    #[test]
    fn the_button_goes_round_one_two_four() {
        let (mut s, g) = studio();
        assert_eq!(s.wave_zoom, 1.0);
        let at = s.center_of(&g, Hit::WaveZoom).unwrap();
        let mut seen = Vec::new();
        for _ in 0..4 {
            assert_eq!(s.hit(&g, at.0, at.1), Hit::WaveZoom);
            assert!(s.press(&g, Hit::WaveZoom));
            seen.push(s.wave_zoom);
        }
        assert_eq!(seen, [2.0, 4.0, 1.0, 2.0]);
        // It sits after the zoom buttons, before the autoscroll switch, clear of the ‹.
        let r = |h| g.toggle(h).unwrap();
        assert!(r(Hit::ZoomIn).right() <= r(Hit::WaveZoom).x);
        assert!(r(Hit::WaveZoom).right() <= r(Hit::Follow).x);
        assert!(r(Hit::Fold).right() <= r(Hit::ToggleArrange).x);
        assert_eq!(
            crate::hover_tip::text(&s, Hit::WaveZoom).as_deref(),
            Some("波形の縦の拡大 ×2")
        );
    }

    #[test]
    fn alt_and_the_wheel_over_the_lanes_step_it_a_notch_at_a_time() {
        let (mut s, g) = studio();
        let at = (g.lanes.x + 300.0, g.lanes.y + 30.0);
        let (beat_w, scroll) = (s.beat_w, s.scroll);
        assert!(s.window_wheel(&g, at, 0.0, ZOOM_NOTCH_PX, alt(), false));
        assert_eq!(s.wave_zoom, 2.0);
        // A touchpad's small turns add up to a notch.
        for _ in 0..3 {
            s.window_wheel(&g, at, 0.0, ZOOM_NOTCH_PX / 4.0, alt(), false);
        }
        assert_eq!(s.wave_zoom, 2.0);
        s.window_wheel(&g, at, 0.0, ZOOM_NOTCH_PX / 4.0, alt(), false);
        assert_eq!(s.wave_zoom, 4.0);
        s.window_wheel(&g, at, 0.0, ZOOM_NOTCH_PX * 3.0, alt(), false);
        assert_eq!(s.wave_zoom, 4.0);
        s.window_wheel(&g, at, 0.0, -ZOOM_NOTCH_PX * 5.0, alt(), false);
        assert_eq!(s.wave_zoom, 1.0);
        // Only the waveforms: the time zoom and the view stay.
        assert_eq!((s.beat_w, s.scroll), (beat_w, scroll));
    }

    #[test]
    fn the_command_sets_steps_and_goes_round() {
        let (mut s, g) = studio();
        let run = |s: &mut Studio, args: serde_json::Value| {
            let args = args.as_object().cloned().unwrap_or_default();
            assert!(s.zoom_command(&g, "view.zoom.wave", &args));
            s.wave_zoom
        };
        assert_eq!(run(&mut s, serde_json::json!({ "to": 4 })), 4.0);
        assert_eq!(run(&mut s, serde_json::json!({ "by": -1 })), 2.0);
        assert_eq!(run(&mut s, serde_json::json!({ "by": -5 })), 1.0);
        assert_eq!(run(&mut s, serde_json::json!({ "to": 1.8 })), 2.0);
        assert_eq!(run(&mut s, serde_json::json!({})), 4.0);
        assert_eq!(run(&mut s, serde_json::json!({})), 1.0);
    }
}
