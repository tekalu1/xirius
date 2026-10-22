//! Scenes for the screens fixed on 2026-10-21 that no earlier script could take: the left
//! rail's balloons (UI-293), the instrument list (UI-297), the piano roll's arrow (UI-292), and
//! the top and bottom bars' snap switch and CPU meter (UI-302, UI-301). Silent; no OS input.
//! The plugin list is a made-up one and the CPU reading is laid by the script (`Perf::fake`),
//! so nothing of this PC is asked. Each script is taken at the default size, and in the tests at
//! 1600x960 too.
use crate::cpu_list::Fake;
use crate::mixer::MixerHit;
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::{DrawerTab, Geometry, Hit, Studio};
use crate::tools::Mods;
use crate::topbar::BarButton;
use std::collections::BTreeMap;
use xirius_core::id::TrackId;
use xirius_core::model::PluginKind;
use xirius_plugins::{Catalog, Entry};

// ------------------------------------------------------------------------- rail-tips

const fn hover(tab: crate::browser::RailTab) -> Act {
    Act::Hover(Target::Rail(tab))
}

/// UI-293: each of the rail's four buttons with the balloon it shows under the pointer.
pub const RAIL_TIPS: &[Shot] = &[
    Shot {
        name: "rt01-instruments",
        acts: &[Act::Reset, hover(crate::browser::RailTab::Instruments)],
    },
    Shot {
        name: "rt02-effects",
        acts: &[hover(crate::browser::RailTab::Effects)],
    },
    Shot {
        name: "rt03-files",
        acts: &[hover(crate::browser::RailTab::Files)],
    },
    Shot {
        name: "rt04-pool",
        acts: &[hover(crate::browser::RailTab::SongFiles)],
    },
];

// ------------------------------------------------------------------- instrument-menu

const fn menu(n: u8) -> Act {
    Act::Stage(Play::Laid(menu_stage), n)
}

/// UI-297: the list a strip without an instrument opens, over a made-up scan of 40 instruments.
pub const INSTRUMENT_MENU: &[Shot] = &[
    // The search box on top, twelve rows, the thumb of a list that goes on.
    Shot {
        name: "im01-list",
        acts: &[Act::Reset, menu(0)],
    },
    // A long name is cut with 「…」 in its row and said whole in the balloon.
    Shot {
        name: "im02-long-name-balloon",
        acts: &[Act::Hover(Target::PopupTip(LONG))],
    },
    // Wheeled to the end: the thumb at the bottom.
    Shot {
        name: "im03-end",
        acts: &[menu(1)],
    },
    // 「pad」 typed in the search box.
    Shot {
        name: "im04-typed",
        acts: &[menu(2)],
    },
];

/// How the long names of the made-up scan start.
const LONG: &str = "Cinematic Hybrid";

/// Made-up instruments: every fifth has a long name, the next a short one with 「Pad」.
fn scan() -> Catalog {
    const MAKERS: [&str; 3] = ["Example Audio", "Sample Works", "Test Sounds"];
    Catalog {
        entries: (0..40)
            .map(|i| Entry {
                uid: format!("{i:032X}"),
                name: match i % 5 {
                    0 => format!("{LONG} Orchestral Strings Ensemble {i}"),
                    1 => format!("Warm Pad {i}"),
                    _ => format!("Synth {i}"),
                },
                vendor: MAKERS[i % MAKERS.len()].into(),
                kind: PluginKind::Instrument,
                format: "vst3".into(),
                category: "Instrument|Synth".into(),
                version: "1.0".into(),
                path: std::path::PathBuf::from(format!("C:/nowhere/{i}.vst3")),
                ara: false,
            })
            .collect(),
        failures: Vec::new(),
    }
}

fn keys(s: &Studio) -> Option<TrackId> {
    let p = s.project();
    p.tracks.values().find(|t| t.name == "Keys").map(|t| t.id)
}

fn menu_stage(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            let g = &s.settled_geometry(g.size.0, g.size.1);
            s.resized(g);
            s.use_plugins(scan());
            let Some(t) = keys(s) else {
                return;
            };
            let hit = Hit::Mixer(MixerHit::AddInstrument(t));
            if s.mixer_center(g, MixerHit::AddInstrument(t)).is_some() {
                s.press(g, hit);
            }
        }
        1 => {
            s.hover = Hit::None;
            if let Some(p) = &mut s.popup {
                p.offset = p.max_offset();
            }
        }
        _ => {
            s.hover = Hit::None;
            if let Some(p) = &mut s.popup {
                p.offset = 0;
            }
            for ch in "pad".chars() {
                let ch = ch.to_string();
                s.fx_typed(&ch, Some(&ch));
            }
        }
    }
}

// ------------------------------------------------------------------------ roll-arrow

const fn arrow(n: u8) -> Act {
    Act::Stage(Play::Laid(arrow_stage), n)
}

/// UI-292: the piano roll's arrow on a note's top edge (velocity) and bottom edge (split). In
/// the basic arrow (the default) the edge shows no band; in the extended one it does.
pub const ROLL_ARROW: &[Shot] = &[
    Shot {
        name: "ra01-basic-top-edge",
        acts: &[Act::Reset, arrow(0), arrow(2)],
    },
    Shot {
        name: "ra02-extended-top-edge",
        acts: &[Act::Reset, arrow(1), arrow(2)],
    },
    Shot {
        name: "ra03-extended-bottom-edge",
        acts: &[arrow(3)],
    },
    Shot {
        name: "ra04-extended-middle",
        acts: &[arrow(4)],
    },
];

/// The pointer on the first of the three notes: its tick, pitch and the share of its height.
fn note_point(s: &Studio, g: &Geometry, f: f32) -> Option<(f32, f32)> {
    let c = s.roll_ctx(g)?;
    let v = s.roll.view(&c);
    Some((v.x(1440), v.y(60) + v.row_h * f))
}

fn arrow_stage(s: &mut Studio, g: &Geometry, n: u8) {
    if n < 2 {
        let mode = if n == 0 { "basic" } else { "extended" };
        crate::edit3_shots::setup_roll_in(s, g, mode);
        return;
    }
    // The edges are 4 px: the top one is the first, the bottom one the last.
    let f = match n {
        2 => 1.0 / 24.0,
        3 => 23.0 / 24.0,
        _ => 0.5,
    };
    let g = &s.settled_geometry(g.size.0, g.size.1);
    let Some((x, y)) = note_point(s, g, f) else {
        return;
    };
    let Some(ctx) = s.roll_input(g) else {
        return;
    };
    let out = s.roll.moved(&ctx, x, y, Mods::NONE);
    s.apply_roll(g, out);
}

// ------------------------------------------------------------------------ bar-states

/// UI-302: the snap switch on and switched off (a slash through the magnet). UI-301: the
/// CPU meter at a low, a high and an over-full reading, in one colour and one shape.
pub const BAR_STATES: &[Shot] = &[
    Shot {
        name: "bs01-snap-on",
        acts: &[Act::Reset],
    },
    Shot {
        name: "bs02-snap-off",
        acts: &[
            Act::Press(Target::Top(BarButton::Snap)),
            Act::Stage(Play::Studio(bar_stage), 0),
        ],
    },
    Shot {
        name: "bs03-cpu-low",
        acts: &[Act::Reset, Act::Stage(Play::Studio(bar_stage), 1)],
    },
    Shot {
        name: "bs04-cpu-high",
        acts: &[Act::Reset, Act::Stage(Play::Studio(bar_stage), 2)],
    },
    Shot {
        name: "bs05-cpu-over",
        acts: &[Act::Reset, Act::Stage(Play::Studio(bar_stage), 3)],
    },
];

/// The load the CPU meter says in stages 1 to 3.
const LOADS: [f64; 3] = [0.12, 0.97, 1.35];

fn bar_stage(s: &mut Studio, n: u8) {
    if n == 0 {
        // The pointer stays on the switch in the window; the picture is of the switch at rest.
        s.hover = Hit::None;
        return;
    }
    s.perf.fake = Some(Fake {
        load: LOADS[n as usize - 1],
        xruns: 0,
        loads: BTreeMap::new(),
    });
    s.poll_perf_now();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::{Look, PopupKind};
    use xirius_design::Layout;

    /// The shots up to and including `name`, played without a window at `w` x `h`.
    fn play_to(shots: &[Shot], name: &str, w: f32, h: f32) -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let body = h - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H;
        let mut g = s.geometry(w, body);
        for shot in shots {
            for act in shot.acts {
                g = s.geometry(w, body);
                match *act {
                    Act::Reset => s = Studio::sample(),
                    Act::Stage(play, n) => {
                        assert!(play.without_window(&mut s, &g, n), "{}", shot.name);
                    }
                    Act::Hover(t) | Act::Press(t) => {
                        let hit = crate::shot::resolve(&s, t)
                            .unwrap_or_else(|| panic!("{}: {t:?}", shot.name));
                        let (x, y) = s
                            .center_of(&g, hit)
                            .unwrap_or_else(|| panic!("{}: {t:?} off screen", shot.name));
                        let actual = s.hit(&g, x, y);
                        assert_eq!(actual, hit, "{}: {t:?}", shot.name);
                        if matches!(act, Act::Press(_)) {
                            s.press(&g, actual);
                        } else {
                            s.hover = actual;
                        }
                    }
                    _ => unreachable!("{}", shot.name),
                }
                assert_eq!(s.error, None, "{}", shot.name);
            }
            if shot.name == name {
                return (s, g);
            }
        }
        panic!("no shot {name}");
    }

    #[test]
    fn the_four_scripts_are_in_the_list_of_scripts() {
        for (script, shots) in [
            ("rail-tips", RAIL_TIPS),
            ("instrument-menu", INSTRUMENT_MENU),
            ("roll-arrow", ROLL_ARROW),
            ("bar-states", BAR_STATES),
        ] {
            let listed = crate::SCRIPTS.iter().find(|(name, _)| *name == script);
            assert!(
                listed.is_some_and(|(_, s)| std::ptr::eq(*s, shots)),
                "{script}"
            );
        }
    }

    #[test]
    fn rail_tips_hover_each_button_and_say_its_tab_and_key() {
        let expect = [
            "インストゥルメント  F6",
            "エフェクト  F7",
            "ファイル  F9",
            "プール",
        ];
        for (shot, text) in RAIL_TIPS.iter().zip(expect) {
            let (s, _) = play_to(RAIL_TIPS, shot.name, 1440.0, 860.0);
            let Hit::Rail(_) = s.hover else {
                panic!("{}: the pointer is on {:?}", shot.name, s.hover);
            };
            assert_eq!(crate::hover_tip::text(&s, s.hover).as_deref(), Some(text));
        }
    }

    #[test]
    fn the_instrument_list_has_a_search_box_cut_names_and_a_thumb_and_narrows_by_typing() {
        for (w, h) in [(1440.0, 860.0), (1600.0, 960.0)] {
            let (s, g) = play_to(INSTRUMENT_MENU, "im01-list", w, h);
            let p = s.popup.as_ref().expect("the list is open");
            assert!(matches!(p.kind, PopupKind::Instrument(_)), "{:?}", p.kind);
            let layout = p.layout(g.size);
            assert_eq!(layout.cells[0].look, Look::Input);
            assert_eq!(layout.cells[0].icon, Some("search"));
            let rows = layout.cells.iter().filter(|c| c.look == Look::Row).count();
            assert_eq!(rows, crate::popup::PICK_ROWS);
            assert!(rows < p.choices.len(), "more rows than show");
            assert!(layout.cells.iter().any(|c| c.look == Look::Thumb));
            assert!(
                layout.cells.iter().any(|c| c.tip.starts_with(LONG)),
                "a cut name"
            );
            assert!(layout.rect.right() <= g.size.0, "inside the window at {w}");
            assert!(layout.rect.bottom() <= g.size.1, "inside the window at {h}");

            let (s, g) = play_to(INSTRUMENT_MENU, "im02-long-name-balloon", w, h);
            let Hit::Item(i) = s.hover else {
                panic!("the pointer is on {:?}", s.hover);
            };
            let cell = &s.popup.as_ref().unwrap().layout(g.size).cells[i];
            assert!(cell.tip.starts_with(LONG));
            assert_eq!(cell.tip, cell.text);

            let (s, _) = play_to(INSTRUMENT_MENU, "im03-end", w, h);
            let p = s.popup.as_ref().unwrap();
            assert!(p.offset > 0 && p.offset == p.max_offset());

            let (s, _) = play_to(INSTRUMENT_MENU, "im04-typed", w, h);
            let p = s.popup.as_ref().unwrap();
            let found: Vec<&str> = crate::fx_picker::found(p)
                .map(|c| c.label.as_str())
                .collect();
            assert_eq!(p.offset, 0);
            assert!(found.len() >= 2, "{found:?}");
            assert!(found.iter().all(|l| l.to_lowercase().contains("pad")));
        }
    }

    #[test]
    fn the_made_up_scan_names_no_product() {
        let scan = scan();
        let names: Vec<&str> = scan.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.iter().any(|n| n.len() > 40), "long names");
        assert!(scan.entries.iter().all(|e| e.vendor.contains("Example")
            || e.vendor.contains("Sample")
            || e.vendor.contains("Test")));
    }

    #[test]
    fn the_arrow_shows_a_band_on_an_edge_only_when_extended() {
        for (w, h) in [(1440.0, 860.0), (1600.0, 960.0)] {
            let band = |name: &str| {
                let (s, g) = play_to(ROLL_ARROW, name, w, h);
                let g = s.settled_geometry(g.size.0, g.size.1);
                let c = s.roll_ctx(&g).unwrap();
                (s.roll.extended, s.roll.zone_band(&c).map(|(z, _)| z))
            };
            use crate::roll::Zone;
            assert_eq!(band("ra01-basic-top-edge"), (false, None));
            assert_eq!(band("ra02-extended-top-edge"), (true, Some(Zone::Velocity)));
            assert_eq!(band("ra03-extended-bottom-edge"), (true, Some(Zone::Split)));
            assert_eq!(band("ra04-extended-middle"), (true, None));
        }
    }

    #[test]
    fn the_bars_show_the_snap_off_and_the_cpu_at_the_laid_readings() {
        for (w, h) in [(1440.0, 860.0), (1600.0, 960.0)] {
            let (s, g) = play_to(BAR_STATES, "bs01-snap-on", w, h);
            assert!(s.bar_view().snap);
            // The meter is in the bar, with room for its well.
            assert!(s.bar_rect(&g, BarButton::Cpu).w > 0.0, "{w}");
            let (s, _) = play_to(BAR_STATES, "bs02-snap-off", w, h);
            assert!(!s.bar_view().snap, "{w}");
            assert_eq!(s.hover, Hit::None);
            for (name, cpu) in [
                ("bs03-cpu-low", 12),
                ("bs04-cpu-high", 97),
                ("bs05-cpu-over", 135),
            ] {
                let (s, _) = play_to(BAR_STATES, name, w, h);
                assert_eq!(s.bar_view().cpu, cpu, "{name} at {w}");
            }
        }
    }
}
