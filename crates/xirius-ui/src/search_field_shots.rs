//! `--script search-field`: the shared search field and the text editing on each screen
//! (UI-338, UI-350, BRW-017): the favourite device unfolded, the select-all, the ✖, the
//! name box opened with the name selected, the menu's search, the ring on a keyed row, the
//! field on every list and screen, and nothing found. Run once dark and once with
//! `--light`. Silent; no OS input.

use serde_json::json;

use crate::browser::RailTab;
use crate::settings::field::Field;
use crate::settings::{Do, FieldId, Section, Sub};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Rect, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    // A favourite device unfolded, the same device in the list folded on its own.
    Shot {
        name: "sf01-favourite-open",
        acts: &[Act::Reset, stage(0)],
    },
    // The words selected in the browser's field.
    Shot {
        name: "sf02-select-all",
        acts: &[Act::Reset, stage(1)],
    },
    // The ✖ showing with words, and the empty field with none.
    Shot {
        name: "sf03-clear-shown",
        acts: &[Act::Reset, stage(2)],
    },
    Shot {
        name: "sf04-empty",
        acts: &[Act::Reset, stage(3)],
    },
    // The track's name opened for a new name: all of it selected.
    Shot {
        name: "sf05-rename",
        acts: &[Act::Reset, stage(4)],
    },
    // A track's menu with words in its search: Japanese, and Latin letters under a group.
    Shot {
        name: "sf06-menu-search",
        acts: &[Act::Reset, stage(5)],
    },
    Shot {
        name: "sf06-menu-search-latin",
        acts: &[Act::Reset, stage(19)],
    },
    // Down from the field: the first row has the ring.
    Shot {
        name: "sf07-ring",
        acts: &[Act::Reset, stage(6)],
    },
    // The list of an insert to add, with words.
    Shot {
        name: "sf08-add-list",
        acts: &[Act::Reset, stage(7)],
    },
    // The add list's caret after several letters, Japanese, a mix, and inside the words.
    Shot {
        name: "sf08-add-list-word",
        acts: &[Act::Reset, stage(20)],
    },
    Shot {
        name: "sf08-add-list-japanese",
        acts: &[Act::Reset, stage(21)],
    },
    Shot {
        name: "sf08-add-list-mixed",
        acts: &[Act::Reset, stage(22)],
    },
    Shot {
        name: "sf08-add-list-caret-inside",
        acts: &[Act::Reset, stage(23)],
    },
    // The start screen's search: with words, all selected, nothing found, and down once.
    Shot {
        name: "sf09-home",
        acts: &[Act::Reset, stage(8)],
    },
    Shot {
        name: "sf09-home-select-all",
        acts: &[Act::Reset, stage(16)],
    },
    Shot {
        name: "sf09-home-nothing-found",
        acts: &[Act::Reset, stage(17)],
    },
    Shot {
        name: "sf09-home-down",
        acts: &[Act::Reset, stage(18)],
    },
    // The shortcuts' search in the settings.
    Shot {
        name: "sf10-settings-keys",
        acts: &[Act::Reset, stage(9)],
    },
    // The command palette with words.
    Shot {
        name: "sf11-palette",
        acts: &[Act::Reset, stage(10)],
    },
    // Nothing found.
    Shot {
        name: "sf12-nothing-found",
        acts: &[Act::Reset, stage(11)],
    },
    // The start screen's search, empty.
    Shot {
        name: "sf13-home-empty",
        acts: &[Act::Reset, stage(12)],
    },
    // The palette empty.
    Shot {
        name: "sf14-palette-empty",
        acts: &[Act::Reset, stage(13)],
    },
    // The add list empty.
    Shot {
        name: "sf15-add-list-empty",
        acts: &[Act::Reset, stage(14)],
    },
    // The shortcuts' search empty.
    Shot {
        name: "sf16-settings-keys-empty",
        acts: &[Act::Reset, stage(15)],
    },
];

fn browser(s: &mut Studio) {
    s.browser.open = true;
    s.browser.tab = RailTab::Effects;
    s.browser.typing = true;
    s.input_map.browser_focus = true;
}

/// Reverb among the favourites, as it is for a search that answers it.
fn favourite_reverb(s: &mut Studio) {
    let (id, _) = s.browser.favourite_keys("Reverb");
    if !s.browser.favourites.contains(&id) {
        s.browser.toggle_favourite("Reverb");
    }
}

fn insert_list(s: &mut Studio, g: &Geometry) {
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    let g = &s.geometry(g.size.0, g.size.1);
    s.resized(g);
    let vocal = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .map(|t| t.id);
    if let Some(t) = vocal {
        s.mixer_press(g, crate::mixer::MixerHit::AddInsert(t));
    }
}

fn keys_page(s: &mut Studio, g: &Geometry) {
    s.open_settings(Section::Keys);
    s.settings_do(g, Do::Sub(Sub::Keyboard), Rect::default());
    // The name search, not the capture of a key press.
    s.settings_do(g, Do::KeyMode(false), Rect::default());
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        // The favourite unfolded.
        0 => {
            s.run_command("preset.factory.restore", json!({}));
            for _ in 0..3000 {
                if s.saving.is_none() {
                    break;
                }
                s.poll_saving();
                s.poll_saving_transport();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // The window's read of the presets is a worker's: wait for its answer.
            let reverb = xirius_params::find("Reverb").map_or("", |d| d.uid);
            for _ in 0..300 {
                s.refresh_presets();
                if s.browser.presets.contains_key(reverb) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // The list of every preset, asked for after the restore dropped the last one.
            s.run_command("browser.preset.list", json!({}));
            for _ in 0..3000 {
                if s.saving.is_none() && s.browser.track_library.is_some() {
                    break;
                }
                s.poll_saving();
                s.poll_saving_transport();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            browser(s);
            s.browser.typing = false;
            s.browser.toggle_favourite("Reverb");
            // Its row's own fold key, so the same device in the list below stays as it is.
            let keys: Vec<_> = s
                .browser
                .track_rows()
                .into_iter()
                .filter(|r| r.branch && r.key.starts_with("favourites/device/"))
                .filter(|r| r.entry.as_ref().is_some_and(|e| e.name == "Reverb"))
                .map(|r| r.key)
                .collect();
            s.browser.opened_groups.extend(keys);
        }
        1 => {
            browser(s);
            favourite_reverb(s);
            s.browser.search.set("Rev".into());
            s.browser.search.select_all();
        }
        2 => {
            browser(s);
            favourite_reverb(s);
            s.browser.search.set("Rev".into());
        }
        3 => {
            browser(s);
            favourite_reverb(s);
        }
        4 => {
            let t = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Bass")
                .map(|t| t.id);
            if let Some(track) = t {
                s.select_row(Some(RowKind::Track(track)));
                s.input_map.name = Some(crate::track_name::Name {
                    track,
                    field: Field::selected(FieldId::Search, "Bass".into()),
                });
            }
        }
        5 | 19 | 6 => {
            if n == 5 || n == 19 {
                track_menu(s, g);
                s.ask_text(None, if n == 5 { "コピー" } else { "midi" }, false);
            } else {
                browser(s);
                favourite_reverb(s);
                s.browser.search.set("Rev".into());
                let g = &s.geometry(g.size.0, g.size.1);
                s.browser_list_key(g, "down", Mods::NONE);
            }
        }
        7 => {
            insert_list(s, g);
            s.fx_typed("r", Some("r"));
        }
        8 => {
            home(s, g);
            s.pick_text(None, "夜", false);
        }
        20..=23 => {
            insert_list(s, g);
            let word = ["reverb", "夜の", "Ab夜c", "Reverb"][usize::from(n) - 20];
            s.pick_text(None, word, false);
            if n == 23 {
                for _ in 0..3 {
                    s.fx_typed("left", None);
                }
            }
        }
        16 => {
            home(s, g);
            s.pick_text(None, "Dem", false);
            if let Some(h) = s.home.as_mut() {
                h.pick.field.select_all();
            }
        }
        17 => {
            home(s, g);
            s.pick_text(None, "qqqqqq", false);
        }
        18 => {
            home(s, g);
            if let Some(h) = s.home.as_mut() {
                h.focus = crate::home::Focus::Search;
            }
            s.home_key(g, "down", Mods::NONE);
        }
        9 => {
            keys_page(s, g);
            s.settings_text(None, "clip", false);
        }
        10 => {
            s.open_palette(g);
            s.ime_replace(None, "ミュート", false);
        }
        11 => {
            browser(s);
            s.browser.tab = RailTab::Instruments;
            s.browser.search.set("qqqqqq".into());
        }
        12 => {
            home(s, g);
            if let Some(h) = s.home.as_mut() {
                h.focus = crate::home::Focus::Search;
            }
        }
        13 => s.open_palette(g),
        14 => insert_list(s, g),
        15 => keys_page(s, g),
        _ => {}
    }
}

/// Three songs saved before, the newest first, in a folder of the shots' own.
fn remember(s: &mut Studio) {
    let root = std::env::temp_dir().join(format!("xirius-search-shots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let keep = crate::keep::Keep::at(root.join("keep"));
    let now = xirius_core::store::now_ms();
    for (name, ago_ms) in [
        ("夜の散歩", 9 * 86_400_000),
        ("朝の曲", 4 * 86_400_000),
        ("Demo", 2 * 3_600_000),
    ] {
        let place = root.join("songs").join(name);
        let mut song = crate::song::Song::empty();
        let _ = song.run("project.rename", json!({ "name": name }));
        let _ = song.save_as(&place);
        keep.remember(&place, name, false, now - ago_ms);
    }
    s.keep = keep;
    s.keep.sync(&s.song.project());
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while s.keep.listed().len() < 3 && std::time::Instant::now() < end {
        s.keep.poll_recent();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn home(s: &mut Studio, g: &Geometry) {
    // The start screen shows over a song with nothing in it.
    *s = Studio::new(crate::song::Song::empty());
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    s.resized(g);
    remember(s);
    s.home = Some(crate::home::Home::default());
    s.home_tick();
}

fn track_menu(s: &mut Studio, g: &Geometry) {
    let g = &s.geometry(g.size.0, g.size.1);
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Bass")
        .map(|t| t.id);
    let Some(row) = t.and_then(|t| s.tree().row_of(RowKind::Track(t))) else {
        return;
    };
    let y = g.column.y + row.y - s.scroll.1 + row.h / 2.0;
    s.scene_context(g, g.column.x + 40.0, y);
}
