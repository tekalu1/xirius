//! The edit tools and the snap grid, shared by the top bar and the piano roll
//! (EDT-001, EDT-016, EDT-017).
//!
//! A tool picked with a number key or in the bar is the same tool in the
//! roll, and the grid chosen in the bar is the grid of the roll's vertical
//! lines. The grid value and snap on/off are the session's view state
//! (`view.grid`, `view.snap`); the studio keeps only the picked tool and the
//! snap mode, and [`Tools`] is the snapshot of all of it that the roll reads.
//! Everything is plain data with no window in it, so it is tested headless.

use xirius_core::time::Tick;

/// The seven tools, in the order of the number keys (7 is left empty).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Select,
    Range,
    Cut,
    Eraser,
    Draw,
    Mute,
    Audition,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::Select,
        Tool::Range,
        Tool::Cut,
        Tool::Eraser,
        Tool::Draw,
        Tool::Mute,
        Tool::Audition,
    ];

    /// The number key that picks the tool.
    pub fn digit(self) -> char {
        match self {
            Tool::Select => '1',
            Tool::Range => '2',
            Tool::Cut => '3',
            Tool::Eraser => '4',
            Tool::Draw => '5',
            Tool::Mute => '6',
            Tool::Audition => '8',
        }
    }

    pub fn from_key(key: &str) -> Option<Tool> {
        let mut chars = key.chars();
        let (c, rest) = (chars.next()?, chars.next());
        if rest.is_some() {
            return None;
        }
        Tool::ALL.into_iter().find(|t| t.digit() == c)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "選択",
            Tool::Range => "範囲",
            Tool::Cut => "分割",
            Tool::Eraser => "消しゴム",
            Tool::Draw => "鉛筆",
            Tool::Mute => "ミュート",
            Tool::Audition => "試聴",
        }
    }
}

/// Which editing area a tool is used in. The same press means different things
/// in the arrangement and in the roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Arrange,
    Roll,
}

/// The modifier keys held right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };

    pub const fn ctrl() -> Mods {
        Mods {
            ctrl: true,
            ..Mods::NONE
        }
    }

    pub const fn shift() -> Mods {
        Mods {
            shift: true,
            ..Mods::NONE
        }
    }

    pub const fn alt() -> Mods {
        Mods {
            alt: true,
            ..Mods::NONE
        }
    }

    pub const fn ctrl_shift() -> Mods {
        Mods {
            ctrl: true,
            shift: true,
            alt: false,
        }
    }

    pub const fn alt_shift() -> Mods {
        Mods {
            alt: true,
            shift: true,
            ctrl: false,
        }
    }
}

/// The default hold from the mouse preset; custom holds are resolved by `Holds`.
pub fn effective_tool(area: Area, tool: Tool, mods: Mods) -> Tool {
    xirius_prefs::Mousemap::default()
        .hold(area.name(), tool.name(), crate::input_dispatch::bits(mods))
        .as_deref()
        .and_then(Tool::named)
        .unwrap_or(tool)
}

/// The holds a person changed in the settings (KEY-026): for each area and
/// picked tool whose holds were changed, the tool each combination of Ctrl,
/// Alt and Shift switches to. A tool not changed keeps [`effective_tool`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Holds([[Option<[Tool; 8]>; 7]; 2]);

impl Holds {
    /// Reads the changed holds of `map` (`<area>.<tool>.hold`).
    pub fn of(map: &xirius_prefs::Mousemap) -> Holds {
        use xirius_prefs::chord::{ALT, MOD, SHIFT};
        let mut holds = Holds::default();
        for area in [Area::Arrange, Area::Roll] {
            for tool in Tool::ALL {
                let mut to = [tool; 8];
                for (i, t) in to.iter_mut().enumerate() {
                    let mut bits = 0;
                    for (bit, m) in [(1, MOD), (2, ALT), (4, SHIFT)] {
                        if i & bit != 0 {
                            bits |= m;
                        }
                    }
                    if let Some(name) = map.hold(area.name(), tool.name(), bits) {
                        *t = Tool::named(&name).unwrap_or(tool);
                    }
                }
                holds.0[area as usize][tool as usize] = Some(to);
            }
        }
        holds
    }

    /// The tool a press acts as, as [`effective_tool`] with the changed holds.
    pub fn tool(&self, area: Area, tool: Tool, mods: Mods) -> Tool {
        let resolved = match self.0[area as usize][tool as usize] {
            Some(to) => {
                to[usize::from(mods.ctrl)
                    | usize::from(mods.alt) << 1
                    | usize::from(mods.shift) << 2]
            }
            None => effective_tool(area, tool, mods),
        };
        if area == Area::Arrange
            && mods.ctrl
            && !mods.alt
            && !mods.shift
            && tool != Tool::Select
            && resolved == tool
        {
            Tool::Select
        } else {
            resolved
        }
    }
}

impl Area {
    /// The name in the mouse settings (`roll`, `arrange`).
    pub fn name(self) -> &'static str {
        match self {
            Area::Arrange => "arrange",
            Area::Roll => "roll",
        }
    }
}

impl Tool {
    /// The name in the mouse settings (`select`, `erase`, `listen` ...).
    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Range => "range",
            Tool::Cut => "cut",
            Tool::Eraser => "erase",
            Tool::Draw => "draw",
            Tool::Mute => "mute",
            Tool::Audition => "listen",
        }
    }

    pub fn named(name: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// How the grid step is found (the top bar's mode list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapMode {
    /// The finest of 1/4 … 1/64 that is still wide enough on screen (the
    /// whole beat when none is).
    Auto,
    /// A whole bar.
    Bar,
    /// The grid value chosen in the list.
    Quantize,
    /// A video frame (30 fps).
    Frames,
}

impl SnapMode {
    pub const ALL: [SnapMode; 4] = [
        SnapMode::Auto,
        SnapMode::Bar,
        SnapMode::Quantize,
        SnapMode::Frames,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SnapMode::Auto => "自動",
            SnapMode::Bar => "小節",
            SnapMode::Quantize => "クオンタイズ",
            SnapMode::Frames => "フレーム",
        }
    }
}

/// What a grid step needs to know about the song and the view.
#[derive(Clone, Copy, Debug)]
pub struct Meter {
    pub ppq: u32,
    pub beats_per_bar: u32,
    pub bpm: f64,
    /// Pixels per beat in the view that is snapping.
    pub beat_px: f32,
}

/// The pixel width a grid step must have for `Auto` to use it.
pub const AUTO_MIN_PX: f32 = 7.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub on: bool,
    pub mode: SnapMode,
    /// The grid value of the bar's list, in ticks (the session's `view.grid`).
    pub grid: Tick,
}

impl Default for Snap {
    fn default() -> Snap {
        Snap {
            on: true,
            mode: SnapMode::Auto,
            grid: 240,
        }
    }
}

impl Snap {
    /// The grid step in ticks, whether snapping is on or not: the roll's
    /// vertical lines, the default length of a drawn note, the nudge.
    pub fn step(&self, m: &Meter) -> Tick {
        let ppq = f64::from(m.ppq);
        let ticks = match self.mode {
            SnapMode::Bar => f64::from(m.beats_per_bar) * ppq,
            SnapMode::Frames => ppq * m.bpm / 60.0 / 30.0,
            SnapMode::Quantize => self.grid as f64,
            SnapMode::Auto => {
                // Finer steps appear as the view widens, down to 1/64.
                let beats = [0.015625, 0.03125, 0.0625, 0.125, 0.25, 0.5, 1.0]
                    .into_iter()
                    .find(|b| (*b as f32) * m.beat_px >= AUTO_MIN_PX)
                    .unwrap_or(1.0);
                beats * ppq
            }
        };
        (ticks.round() as Tick).max(1)
    }

    /// The step a pointer snaps to: none when snapping is off or the key that
    /// turns it off for this drag is held (Shift).
    pub fn active(&self, m: &Meter, shift: bool) -> Option<Tick> {
        (self.on != shift).then(|| self.step(m))
    }
}

/// Rounds `value` to the nearest multiple of `step`.
pub fn snap_round(value: Tick, step: Tick) -> Tick {
    if step <= 1 {
        return value;
    }
    (value as f64 / step as f64).round() as Tick * step
}

/// Rounds `value` down to a multiple of `step`.
pub fn snap_floor(value: Tick, step: Tick) -> Tick {
    if step <= 1 {
        return value;
    }
    value.div_euclid(step) * step
}

/// The picked tool and the grid, as the roll sees them for one event or one
/// frame (`Studio::tools`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tools {
    pub tool: Tool,
    pub snap: Snap,
    /// The holds changed in the settings (KEY-026).
    pub holds: Holds,
}

impl Tools {
    /// The tool a press in `area` acts as with `mods` held.
    pub fn held(&self, area: Area, mods: Mods) -> Tool {
        self.holds.tool(area, self.tool, mods)
    }
}

impl Default for Tools {
    fn default() -> Tools {
        Tools {
            tool: Tool::Select,
            snap: Snap::default(),
            holds: Holds::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Meter = Meter {
        ppq: 960,
        beats_per_bar: 4,
        bpm: 120.0,
        beat_px: 48.0,
    };

    #[test]
    fn number_keys_pick_the_tools_and_seven_is_empty() {
        for (key, tool) in [
            ("1", Tool::Select),
            ("2", Tool::Range),
            ("3", Tool::Cut),
            ("4", Tool::Eraser),
            ("5", Tool::Draw),
            ("6", Tool::Mute),
            ("8", Tool::Audition),
        ] {
            assert_eq!(Tool::from_key(key), Some(tool));
        }
        for key in ["7", "9", "a", "12"] {
            assert_eq!(Tool::from_key(key), None);
        }
    }

    #[test]
    fn in_the_roll_ctrl_is_a_pencil_and_ctrl_shift_a_cut() {
        use Area::Roll;
        assert_eq!(effective_tool(Roll, Tool::Select, Mods::NONE), Tool::Select);
        assert_eq!(effective_tool(Roll, Tool::Select, Mods::ctrl()), Tool::Draw);
        assert_eq!(
            effective_tool(Roll, Tool::Select, Mods::ctrl_shift()),
            Tool::Cut
        );
        assert_eq!(
            effective_tool(Roll, Tool::Select, Mods::shift()),
            Tool::Select
        );
        for tool in [Tool::Draw, Tool::Cut, Tool::Eraser] {
            assert_eq!(effective_tool(Roll, tool, Mods::ctrl()), Tool::Select);
            assert_eq!(effective_tool(Roll, tool, Mods::NONE), tool);
        }
        assert_eq!(effective_tool(Roll, Tool::Mute, Mods::ctrl()), Tool::Mute);
    }

    #[test]
    fn in_the_arrangement_ctrl_is_range_and_alt_gives_select_back() {
        use Area::Arrange;
        assert_eq!(
            effective_tool(Arrange, Tool::Select, Mods::ctrl()),
            Tool::Range
        );
        for tool in [Tool::Range, Tool::Cut, Tool::Eraser, Tool::Mute] {
            assert_eq!(effective_tool(Arrange, tool, Mods::alt()), Tool::Select);
        }
        assert_eq!(effective_tool(Arrange, Tool::Draw, Mods::alt()), Tool::Draw);
    }

    #[test]
    fn the_snap_modes_find_their_step() {
        let mut s = Snap::default();
        assert_eq!(s.step(&M), 240);
        s.mode = SnapMode::Bar;
        assert_eq!(s.step(&M), 3840);
        s.mode = SnapMode::Frames;
        // 120 bpm: a beat is 0.5 s, a 30 fps frame is 1/30 s.
        assert_eq!(s.step(&M), 64);
        s.mode = SnapMode::Auto;
        // 48 px a beat: 1/64 (3 px) and 1/32 (6 px) are too fine, 1/16 (12 px) is wide enough.
        assert_eq!(s.step(&M), 240);
        let wide = Meter { beat_px: 96.0, ..M };
        assert_eq!(s.step(&wide), 120);
        let narrow = Meter { beat_px: 6.0, ..M };
        assert_eq!(s.step(&narrow), 960);
    }

    #[test]
    fn the_grid_gets_finer_as_the_view_widens_down_to_1_64() {
        let s = Snap::default();
        // At 447 px a beat 1/64 is 6.98 px, under the 7 px minimum, so 1/32 shows;
        // at 448 px 1/64 is exactly 7 px and shows.
        let at = |beat_px: f32| s.step(&Meter { beat_px, ..M });
        assert_eq!(at(447.0), 30);
        assert_eq!(at(448.0), 15);
        assert_eq!(at(4096.0), 15);
        let mut last = Tick::MAX;
        for beat_px in [48.0, 96.0, 192.0, 384.0, 768.0, 1536.0, 4096.0] {
            let step = at(beat_px);
            assert!(step <= last, "{beat_px}: {step} after {last}");
            last = step;
        }
    }

    #[test]
    fn shift_flips_snapping_for_one_drag() {
        let mut s = Snap::default();
        assert_eq!(s.active(&M, false), Some(240));
        assert_eq!(s.active(&M, true), None);
        s.on = false;
        assert_eq!(s.active(&M, false), None);
        assert_eq!(s.active(&M, true), Some(240));
    }

    #[test]
    fn rounding_to_a_step() {
        assert_eq!(snap_round(130, 240), 240);
        assert_eq!(snap_round(110, 240), 0);
        assert_eq!(snap_round(-130, 240), -240);
        assert_eq!(snap_floor(-1, 240), -240);
        assert_eq!(snap_floor(479, 240), 240);
        assert_eq!(snap_round(77, 1), 77);
    }
}
