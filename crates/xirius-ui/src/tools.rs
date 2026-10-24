//! The edit tools and the snap grid, shared by the top bar and the piano roll
//! (EDT-001, EDT-016, EDT-017).
//!
//! A tool picked with a number key or in the bar is the same tool in the
//! roll, and the grid chosen in the bar is the grid of the roll's vertical
//! lines. The grid value and snap on/off are the session's view state
//! (`view.grid`, `view.snap`); the studio keeps only the picked tool and the
//! snap mode, and [`Tools`] is the snapshot of all of it that the roll reads.
//! Everything is plain data with no window in it, so it is tested headless.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use xirius_core::model::Project;
use xirius_core::tempo::TempoMap;
use xirius_core::time::{Side, Tick, TimeMap};

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

/// What a grid step needs to know about the song and the view: the meters (a
/// step counts from the start of its bar), the tempo (a frame is found in
/// seconds) and how wide a beat is in the view that is snapping. The maps are
/// shared, so a copy for another view's width costs nothing.
#[derive(Clone, Debug)]
pub struct Meter {
    pub ppq: u32,
    pub time: Arc<TimeMap>,
    pub tempo: Arc<TempoMap>,
    /// Pixels per beat in the view that is snapping.
    pub beat_px: f32,
}

impl Meter {
    /// The song's meters and tempo, seen `beat_px` wide a beat.
    pub fn of(p: &Project, beat_px: f32) -> Meter {
        Meter {
            ppq: p.ppq,
            time: Arc::new(p.time_map()),
            tempo: Arc::new(TempoMap::from_project(p)),
            beat_px,
        }
    }

    /// The same song seen `beat_px` wide a beat.
    pub fn at(&self, beat_px: f32) -> Meter {
        Meter {
            beat_px,
            ..self.clone()
        }
    }

    /// 4/4 at 120 bpm throughout.
    pub fn plain(ppq: u32, beat_px: f32) -> Meter {
        Meter {
            ppq,
            time: Arc::new(TimeMap::new(ppq, &[])),
            tempo: Arc::new(TempoMap::new(ppq, &[])),
            beat_px,
        }
    }

    /// How wide `ticks` are in the view, in px.
    pub fn px(&self, ticks: f64) -> f32 {
        (ticks / f64::from(self.ppq.max(1))) as f32 * self.beat_px
    }
}

/// The song's [`Meter`], built again only when the song changes: the views
/// snap with it at every pointer move and draw with it at every frame. A
/// change gives the song a new `Arc`, and the `Weak` held keeps the old one's
/// place from being taken by another.
#[derive(Default)]
pub struct Meters(RefCell<Option<(Weak<Project>, Meter)>>);

impl Meters {
    /// The meters of `p`, seen `beat_px` wide a beat.
    pub fn of(&self, p: &Arc<Project>, beat_px: f32) -> Meter {
        let mut cache = self.0.borrow_mut();
        if let Some((song, m)) = &*cache
            && std::ptr::eq(song.as_ptr(), Arc::as_ptr(p))
        {
            return m.at(beat_px);
        }
        let m = Meter::of(p, beat_px);
        *cache = Some((Arc::downgrade(p), m.clone()));
        m
    }
}

/// The pixel width a grid step must have for `Auto` to use it: the same as the
/// finest grid line shown (UI-339).
pub const AUTO_MIN_PX: f32 = xirius_design::GridLine::MIN_FINE_PX;

/// The frame rate of the `Frames` mode.
pub const FPS: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub on: bool,
    pub mode: SnapMode,
    /// The grid value of the bar's list, in ticks (the session's `view.grid`).
    pub grid: Tick,
    /// The grid value is a triplet (`1/8t`): a beat is split in three.
    pub triplet: bool,
}

impl Default for Snap {
    fn default() -> Snap {
        Snap {
            on: true,
            mode: SnapMode::Auto,
            grid: 240,
            triplet: false,
        }
    }
}

impl Snap {
    /// The grid step in ticks, whether snapping is on or not: the roll's
    /// vertical lines, the default length of a drawn note, the nudge. A bar is
    /// the first bar's length and a frame the first tempo's (as a length; a
    /// position is put on the grid by [`Snap::round`]).
    pub fn step(&self, m: &Meter) -> Tick {
        let ppq = f64::from(m.ppq);
        let ticks = match self.mode {
            SnapMode::Bar => m.time.meter_at(1).bar_ticks(m.ppq) as f64,
            SnapMode::Frames => ppq * m.tempo.bpm_at(0) / 60.0 / FPS,
            SnapMode::Quantize => self.grid as f64,
            SnapMode::Auto => {
                // The menu's values from the 1/64 note to the 1/4: the finest
                // one at least the minimum apart (UI-339).
                let beats = [0.0625, 0.125, 0.25, 0.5, 1.0]
                    .into_iter()
                    .find(|b| (*b as f32) * m.beat_px >= AUTO_MIN_PX)
                    .unwrap_or(1.0);
                beats * ppq
            }
        };
        (ticks.round() as Tick).max(1)
    }

    /// Whether the grid's value splits a beat in three (a triplet value chosen
    /// in the list).
    pub fn triplets(&self) -> bool {
        self.mode == SnapMode::Quantize && self.triplet
    }

    /// The step a pointer snaps to: none when snapping is off or the key that
    /// turns it off for this drag is held (Shift).
    pub fn active(&self, m: &Meter, shift: bool) -> Option<Tick> {
        (self.on != shift).then(|| self.step(m))
    }

    /// `t` on the nearest grid line: steps count from the start of `t`'s bar,
    /// a bar is the nearest bar line and a frame is found in seconds.
    pub fn round(&self, m: &Meter, t: Tick) -> Tick {
        self.put(m, t, Side::Near)
    }

    /// `t` on the grid line at or before it.
    pub fn floor(&self, m: &Meter, t: Tick) -> Tick {
        self.put(m, t, Side::Before)
    }

    /// `t` on the grid line at or after it.
    pub fn ceil(&self, m: &Meter, t: Tick) -> Tick {
        self.put(m, t, Side::After)
    }

    /// [`Snap::round`] when snapping is on for this drag, else `t`.
    pub fn snapped(&self, m: &Meter, shift: bool, t: Tick) -> Tick {
        if self.on != shift {
            self.round(m, t)
        } else {
            t
        }
    }

    /// [`Snap::floor`] when snapping is on for this drag, else `t`.
    pub fn floored(&self, m: &Meter, shift: bool, t: Tick) -> Tick {
        if self.on != shift {
            self.floor(m, t)
        } else {
            t
        }
    }

    fn put(&self, m: &Meter, t: Tick, side: Side) -> Tick {
        match self.mode {
            SnapMode::Frames => {
                let frames = m.tempo.seconds_at(t) * FPS;
                let k = match side {
                    Side::Near => frames.round(),
                    Side::Before => frames.floor(),
                    Side::After => frames.ceil(),
                };
                let at = m.tempo.tick_at(k / FPS);
                // A frame found from a rounded tick may sit a tick to the wrong side.
                match side {
                    Side::Before => at.min(t),
                    Side::After => at.max(t),
                    Side::Near => at,
                }
            }
            SnapMode::Bar => m.time.on_grid(t, Tick::MAX, side),
            _ => m.time.on_grid(t, self.step(m), side),
        }
    }
}

/// `t` on the nearest line of a grid of `step` counted from the start of its
/// bar (a lane's own grid, which may be coarser than the snap's).
pub fn round_in_bars(tm: &TimeMap, t: Tick, step: Tick) -> Tick {
    tm.on_grid(t, step, Side::Near)
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

    fn at(beat_px: f32) -> Meter {
        Meter::plain(960, beat_px)
    }

    /// 4/4 for a bar, then 7/8 from bar 2 and 6/8 from bar 3.
    fn odd(beat_px: f32) -> Meter {
        use xirius_core::time::Meter as Sig;
        let changes = [
            (1, Sig { num: 4, den: 4 }),
            (2, Sig { num: 7, den: 8 }),
            (3, Sig { num: 6, den: 8 }),
        ];
        Meter {
            time: TimeMap::new(960, &changes).into(),
            ..Meter::plain(960, beat_px)
        }
    }

    fn tempo(points: &[(Tick, f64)]) -> TempoMap {
        let points: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(i, &(tick, bpm))| xirius_core::model::TempoPoint {
                id: xirius_core::id::TempoId(i as u64 + 1),
                tick,
                bpm,
                ramp: false,
            })
            .collect();
        TempoMap::new(960, &points)
    }

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
        let m = at(48.0);
        let mut s = Snap::default();
        assert_eq!(s.step(&m), 240);
        s.mode = SnapMode::Bar;
        assert_eq!(s.step(&m), 3840);
        s.mode = SnapMode::Frames;
        // 120 bpm: a beat is 0.5 s, a 30 fps frame is 1/30 s.
        assert_eq!(s.step(&m), 64);
        s.mode = SnapMode::Auto;
        // 48 px a beat: 1/64 (3 px) and 1/32 (6 px) are too fine, 1/16 (12 px) is wide enough.
        assert_eq!(s.step(&m), 240);
        assert_eq!(s.step(&at(96.0)), 120);
        assert_eq!(s.step(&at(6.0)), 960);
    }

    #[test]
    fn a_bar_in_six_eight_is_one_bar() {
        use xirius_core::time::Meter as Sig;
        let m = Meter {
            time: TimeMap::new(960, &[(1, Sig { num: 6, den: 8 })]).into(),
            ..at(48.0)
        };
        let s = Snap {
            mode: SnapMode::Bar,
            ..Snap::default()
        };
        assert_eq!(s.step(&m), 2880);
        assert_eq!(s.round(&m, 1000), 0);
        assert_eq!(s.round(&m, 1500), 2880);
    }

    #[test]
    fn the_grid_gets_finer_as_the_view_widens_down_to_the_64th_note() {
        let s = Snap::default();
        // At 127 px a beat the 1/64 note (60 ticks) is 7.9 px, under the 8 px
        // minimum, so the 1/32 note is the step; at 128 px the 1/64 note is 8 px.
        let step = |beat_px: f32| s.step(&at(beat_px));
        assert_eq!(step(127.0), 120);
        assert_eq!(step(128.0), 60);
        // The 1/64 note is the menu's finest value: the auto step goes no finer.
        assert_eq!(step(4096.0), 60);
        // Zoomed out it stays on the 1/4 note.
        assert_eq!(step(7.9), 960);
        assert_eq!(step(8.0), 960);
        assert_eq!(step(16.0), 480);
        assert_eq!(step(48.0), 240);
        let mut last = Tick::MAX;
        for beat_px in [48.0, 96.0, 192.0, 384.0, 768.0, 1536.0, 4096.0] {
            let step = step(beat_px);
            assert!(step <= last, "{beat_px}: {step} after {last}");
            last = step;
        }
    }

    #[test]
    fn shift_flips_snapping_for_one_drag() {
        let m = at(48.0);
        let mut s = Snap::default();
        assert_eq!(s.active(&m, false), Some(240));
        assert_eq!(s.active(&m, true), None);
        assert_eq!(s.snapped(&m, false, 130), 240);
        assert_eq!(s.snapped(&m, true, 130), 130);
        s.on = false;
        assert_eq!(s.active(&m, false), None);
        assert_eq!(s.active(&m, true), Some(240));
        assert_eq!(s.floored(&m, true, 479), 240);
    }

    #[test]
    fn the_grid_counts_from_the_start_of_each_bar() {
        // 7/8 from bar 2 (3840) and 6/8 from bar 3 (3840 + 3360 = 7200).
        let m = odd(48.0);
        let s = Snap {
            mode: SnapMode::Quantize,
            grid: 320,
            triplet: true,
            ..Snap::default()
        };
        // 1/8t is 320 ticks: 3360 (7/8) is not a multiple of it, but each bar's
        // lines start on its own first tick.
        assert_eq!(s.round(&m, 3840 + 330), 3840 + 320);
        assert_eq!(s.floor(&m, 3840 + 650), 3840 + 640);
        assert_eq!(s.ceil(&m, 3840 + 1), 3840 + 320);
        // The last step of the 7/8 bar is cut short by the next bar line.
        assert_eq!(s.round(&m, 7190), 7200);
        assert_eq!(s.ceil(&m, 3840 + 3201), 7200);
        assert_eq!(s.floor(&m, 7200 + 10), 7200);
        assert_eq!(s.round(&m, 7200 + 330), 7200 + 320);
        // A tick on a line stays.
        assert_eq!(s.round(&m, 7200 + 640), 7200 + 640);
        // From the song's start a 1/16 grid gives the same as before.
        let q = Snap {
            mode: SnapMode::Quantize,
            grid: 240,
            ..Snap::default()
        };
        assert_eq!(q.round(&m, 130), 240);
        assert_eq!(q.round(&m, 110), 0);
        assert_eq!(q.round(&m, -130), -240);
        assert_eq!(q.floor(&m, -1), -240);
        assert_eq!(q.ceil(&m, -1), 0);
    }

    #[test]
    fn a_bar_snaps_to_the_nearest_bar_line_of_its_own_length() {
        let m = odd(48.0);
        let s = Snap {
            mode: SnapMode::Bar,
            ..Snap::default()
        };
        assert_eq!(s.round(&m, 3840 + 1600), 3840);
        assert_eq!(s.round(&m, 3840 + 1700), 7200);
        assert_eq!(s.floor(&m, 7199), 3840);
        assert_eq!(s.ceil(&m, 3841), 7200);
    }

    #[test]
    fn frames_follow_the_tempo_changes() {
        // 120 bpm for 2 beats (1 s), then 60 bpm: a frame is 64 ticks, then 32.
        let m = Meter {
            tempo: tempo(&[(0, 120.0), (1920, 60.0)]).into(),
            ..at(48.0)
        };
        let s = Snap {
            mode: SnapMode::Frames,
            ..Snap::default()
        };
        assert_eq!(s.round(&m, 70), 64);
        assert_eq!(s.round(&m, 1920 + 40), 1920 + 32);
        assert_eq!(s.floor(&m, 1920 + 63), 1920 + 32);
        assert_eq!(s.ceil(&m, 1920 + 1), 1920 + 32);
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
