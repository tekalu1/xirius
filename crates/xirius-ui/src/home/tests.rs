//! The start screen (UI-350): what shows the first time and with songs, the keys, the narrow
//! window, no audio device, and the names a screen reader reads.

use super::*;
use crate::song::Song;
use crate::tools::Mods;
use gpui::accesskit;

/// The start screen with these songs remembered, oldest first (the list shows them newest
/// first), on a PC with one audio device.
fn kept(names: &[&str]) -> (Studio, Geometry, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let keep = crate::keep::Keep::at(tmp.path().join("keep"));
    for (i, name) in names.iter().enumerate() {
        let path = tmp.path().join(name);
        let mut seed = Song::empty();
        seed.save_as(&path).unwrap();
        drop(seed);
        keep.remember(&path, name, false, 1 + i as u64);
    }
    let mut s = Studio::new(Song::empty());
    with_device(&mut s, true);
    s.keep = keep;
    s.keep.sync(&s.song.project());
    s.keep.settle_recent();
    s.keep.poll_recent();
    s.home = Some(Home::default());
    s.home_tick();
    let g = s.geometry(1280.0, 720.0);
    (s, g, tmp)
}

/// One simulated output (`device`), or none: nothing is opened, the output is silent.
fn with_device(s: &mut Studio, device: bool) {
    use xirius_engine::devices::{Device, Simulated};
    struct Quiet;
    impl xirius_engine::record::AudioInput for Quiet {
        fn channels(&self) -> usize {
            2
        }
        fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
            left.fill(0.0);
            right.fill(0.0);
        }
    }
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    let devices = device
        .then(|| Device {
            name: "Speakers".into(),
            inputs: Vec::new(),
            outputs: vec!["Out 1".into(), "Out 2".into()],
        })
        .into_iter()
        .collect();
    s.song
        .io
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(Box::new(Simulated::new(
            devices,
            Default::default(),
            Box::new(Quiet),
        )));
    s.poll_io();
}

fn home(s: &Studio) -> &Home {
    s.home.as_ref().expect("the start screen")
}

fn labels(s: &Studio) -> Vec<String> {
    home(s).pick.nav().iter().map(|r| r.label.clone()).collect()
}

const NONE: Mods = Mods::NONE;

#[test]
fn the_first_time_lists_only_the_sample_with_no_search_and_new_has_the_keys() {
    let (s, g, _tmp) = kept(&[]);
    assert!(s.home_shown());
    assert_eq!(labels(&s), [crate::song::SAMPLE_NAME]);
    assert_eq!(home(&s).focus, Focus::Button(Button::New));
    let l = s.home_layout(&g).unwrap();
    assert_eq!(l.search, None, "nothing to search");
    assert_eq!(l.rows.len(), 1);
    assert!(s.home_typing().is_none(), "letters do not go anywhere");
    assert_eq!(
        home(&s).order(),
        [
            Focus::Button(Button::New),
            Focus::Button(Button::Open),
            Focus::Button(Button::Dawproject),
            Focus::List
        ]
    );
}

#[test]
fn songs_list_newest_first_with_the_sample_last_and_the_top_row_chosen() {
    let (s, g, _tmp) = kept(&["older", "newer"]);
    assert_eq!(labels(&s), ["newer", "older", crate::song::SAMPLE_NAME]);
    assert_eq!(home(&s).focus, Focus::List);
    assert_eq!(home(&s).pick.selected, 0);
    let rows = home(&s).pick.nav();
    assert_eq!(rows.last().unwrap().act, Some(Action::OpenSample));
    let l = s.home_layout(&g).unwrap();
    assert!(l.search.is_some());
    assert_eq!(l.rows.len(), 3);
    // The pointer finds each part where it shows.
    for spot in [
        Spot::Button(Button::New),
        Spot::Button(Button::Open),
        Spot::Button(Button::Dawproject),
        Spot::Search,
        Spot::Row(0),
        Spot::Row(2),
    ] {
        let (x, y) = s.center_of(&g, Hit::Home(spot)).unwrap();
        assert_eq!(s.hit(&g, x, y), Hit::Home(spot));
    }
    // No device line with a device; the bar has only the logo.
    assert_eq!(l.no_device, None);
    assert!(s.bar_layout(g.size.0).is_logo_only());
}

#[test]
fn tab_walks_new_open_dawproject_search_and_the_list() {
    let (mut s, g, _tmp) = kept(&["a"]);
    let tab = Mods::NONE;
    let back = Mods {
        shift: true,
        ..Mods::NONE
    };
    let mut seen = vec![home(&s).focus];
    for _ in 0..5 {
        assert!(s.home_key(&g, "tab", tab));
        seen.push(home(&s).focus);
    }
    assert_eq!(
        seen,
        [
            Focus::List,
            Focus::Button(Button::New),
            Focus::Button(Button::Open),
            Focus::Button(Button::Dawproject),
            Focus::Search,
            Focus::List
        ]
    );
    assert!(s.home_key(&g, "tab", back));
    assert_eq!(home(&s).focus, Focus::Search);
}

#[test]
fn the_arrows_home_and_end_walk_the_list_and_enter_opens_the_row() {
    let (mut s, g, tmp) = kept(&["a", "b", "c"]);
    assert!(s.home_key(&g, "down", NONE));
    assert_eq!(home(&s).pick.selected, 1);
    assert!(s.home_key(&g, "end", NONE));
    assert_eq!(home(&s).pick.selected, 3, "the sample");
    assert!(s.home_key(&g, "home", NONE));
    assert_eq!(home(&s).pick.selected, 0);
    assert!(s.home_key(&g, "down", NONE));
    assert!(s.home_key(&g, "up", NONE));
    assert_eq!(home(&s).pick.selected, 0);
    assert!(s.home_key(&g, "down", NONE));
    assert!(s.home_key(&g, "enter", NONE));
    assert!(s.opening.is_some(), "b opens");
    s.wait_for_open_for_test();
    assert_eq!(s.error, None);
    assert_eq!(s.song.dir(), Some(tmp.path().join("b")));
    assert!(s.home.is_none(), "a song is open: the start screen is done");
}

#[test]
fn a_double_press_on_a_row_opens_it_and_a_single_one_chooses_it() {
    let (mut s, g, _tmp) = kept(&["a"]);
    assert!(s.press(&g, Hit::Home(Spot::Row(1))));
    assert_eq!(home(&s).pick.selected, 1);
    assert!(s.opening.is_none(), "one press only chooses");
    assert!(s.press(&g, Hit::Home(Spot::Row(1))));
    assert!(s.opening.is_some(), "the second press opens the sample");
    s.wait_for_open_for_test();
    assert!(s.song.is_sample());
    assert!(!s.home_shown());
}

#[test]
fn typing_goes_to_the_search_and_esc_clears_it() {
    let (mut s, g, _tmp) = kept(&["apple", "banana"]);
    assert!(s.home_typing().is_some());
    assert!(
        !s.human_interaction_active(),
        "an idle start screen is nobody's work"
    );
    s.pick_text(None, "ban", false);
    assert_eq!(home(&s).focus, Focus::Search);
    assert_eq!(home(&s).pick.query, "ban");
    assert_eq!(labels(&s), ["banana"]);
    assert!(
        s.human_interaction_active(),
        "what is typed is someone's work"
    );
    assert!(s.home_key(&g, "backspace", NONE));
    assert_eq!(home(&s).pick.query, "ba");
    assert!(s.home_key(&g, "escape", NONE));
    assert_eq!(home(&s).pick.query, "");
    assert_eq!(labels(&s).len(), 3);
    // Without a query Esc is the key map's (nothing to clear).
    assert!(!s.home_key(&g, "escape", NONE));
}

/// Only the idle search is let off: any other field the IME writes in over the start screen
/// is someone's work, as everywhere (UI-350 review).
#[test]
fn another_field_over_the_idle_start_screen_is_someones_work() {
    let (mut s, _, _tmp) = kept(&["apple"]);
    assert!(s.home_typing_idle());
    assert!(!s.human_interaction_active());
    s.input_map.name = Some(crate::track_name::Name {
        track: xirius_core::id::TrackId(1),
        field: crate::settings::Field::new(crate::settings::FieldId::Search, String::new()),
    });
    assert!(s.home_typing_idle(), "the search is still empty");
    assert!(s.human_interaction_active(), "a name being typed");
}

#[test]
fn space_and_enter_press_the_focused_button() {
    let (mut s, g, _tmp) = kept(&[]);
    assert!(s.home_key(&g, "down", NONE));
    assert_eq!(home(&s).focus, Focus::Button(Button::Open));
    assert!(s.home_key(&g, "space", NONE));
    assert_eq!(s.file_request, Some(FileRequest::Open));
    s.file_request = None;
    assert!(s.home_key(&g, "down", NONE));
    assert!(s.home_key(&g, "enter", NONE));
    assert_eq!(s.file_request, Some(FileRequest::OpenDawproject));
    s.file_request = None;
    // 新規: the new song's screen, with its three entries.
    assert!(s.press(&g, Hit::Home(Spot::Button(Button::New))));
    s.wait_for_open_for_test();
    assert!(s.start_shown());
    assert!(!s.home_shown());
}

#[test]
fn ctrl_n_and_ctrl_o_work_and_the_songs_keys_do_not() {
    let (mut s, g, _tmp) = kept(&["a"]);
    let ctrl = |k: &str| {
        (
            k.to_string(),
            Mods {
                ctrl: true,
                ..Mods::NONE
            },
        )
    };
    // A song's key (Ctrl+S) does nothing here.
    let (k, m) = ctrl("s");
    assert!(
        !s.home_key(&g, &k, m),
        "the start screen leaves Ctrl's keys"
    );
    assert!(!s.mapped_key_code(&g, &k, None, m, None));
    assert!(s.file_request.is_none());
    let (k, m) = ctrl("o");
    assert!(s.mapped_key_code(&g, &k, None, m, None));
    assert_eq!(s.file_request, Some(FileRequest::Open));
    s.file_request = None;
    let (k, m) = ctrl("n");
    assert!(s.mapped_key_code(&g, &k, None, m, None));
    s.wait_for_open_for_test();
    assert!(!s.home_shown(), "a new song");
}

#[test]
fn a_narrow_window_stacks_one_column_with_the_buttons_in_a_row_without_their_keys() {
    let (s, _, _tmp) = kept(&["a"]);
    let wide = s.geometry(1280.0, 720.0);
    let l = s.home_layout(&wide).unwrap();
    assert!(l.keys);
    let new = l.button(Button::New);
    let open = l.button(Button::Open);
    assert_eq!(new.x, open.x, "one over the other");
    assert!(open.y > new.y);
    assert!(l.heading.x > new.right(), "the list at the right");

    let narrow = s.geometry(480.0, 720.0);
    let l = s.home_layout(&narrow).unwrap();
    assert!(!l.keys, "no keys in a narrow window");
    let new = l.button(Button::New);
    let open = l.button(Button::Open);
    assert_eq!(new.y, open.y, "in a row");
    assert!(open.x > new.right());
    // Narrow is the body under 520 wide (ui-approved), not the body less its margins
    // (UI-350 review).
    assert!(narrow.size.0 < NARROW);
    let keys = |w: f32| s.home_layout(&s.geometry(w, 720.0)).unwrap().keys;
    assert!(!keys(NARROW - 1.0));
    assert!(keys(NARROW), "520 is not under 520");
    let side = Space::X3.px();
    let content = narrow.size.0 - 2.0 * side;
    assert_eq!(l.heading.x, side);
    assert_eq!(l.heading.w, content, "the list at the full width");
    let lowest = l.buttons.iter().map(|r| r.bottom()).fold(0.0, f32::max);
    assert!(l.heading.y > lowest, "under the buttons");
    for b in l.buttons {
        assert!(b.right() <= side + content + 0.5, "{b:?}");
    }
    // The search fills the heading's row after its words.
    let search = l.search.expect("songs to search");
    assert_eq!(search.y + search.h / 2.0, l.heading.y + l.heading.h / 2.0);
    assert!(search.x > l.heading.x);
    assert_eq!(search.right(), l.heading.right());
    for (_, r) in &l.rows {
        assert!(r.y >= l.heading.bottom(), "{r:?}");
    }
}

/// The wide form is the approved mock's: at the top under the bar, the buttons 184 wide and
/// 32 tall, the search 180 wide at the heading's right, and the rows under the heading
/// (UI-350 review).
#[test]
fn a_wide_window_puts_the_content_at_the_top_as_the_mock_has_it() {
    let (s, g, _tmp) = kept(&["a", "b"]);
    let l = s.home_layout(&g).unwrap();
    let new = l.button(Button::New);
    assert_eq!(new.y, TOP, "at the top, not in the middle");
    assert_eq!((new.w, new.h), (LEFT_W, BUTTON_H));
    let open = l.button(Button::Open);
    let daw = l.button(Button::Dawproject);
    assert!(open.y - new.bottom() > daw.y - open.bottom(), "新規 apart");
    assert_eq!(l.heading.y, TOP);
    assert!(l.heading.x >= new.right() + Space::X6.px());
    assert!(l.heading.w <= CONTENT_MAX);
    let search = l.search.expect("songs to search");
    assert_eq!(search.w, SEARCH_W);
    assert_eq!(search.right(), l.heading.right(), "at the heading's right");
    assert!(search.y >= l.heading.y && search.bottom() <= l.heading.bottom());
    assert!(!l.cells.iter().any(|c| matches!(c.look, Look::Input)));
    let (_, first) = l.rows[0];
    assert!(first.y >= l.heading.bottom(), "the rows under the heading");
    assert_eq!(first.x, l.heading.x);
    assert_eq!(first.w, l.heading.w);
    // The search box is pressed where it is drawn.
    let (x, y) = (search.x + search.w / 2.0, search.y + search.h / 2.0);
    assert_eq!(s.hit(&g, x, y), Hit::Home(Spot::Search));
}

/// The smallest window the app opens (960×600, UI-300) keeps the two columns, and every
/// part inside the body.
#[test]
fn the_smallest_window_keeps_two_columns_inside_the_body() {
    let (s, _, _tmp) = kept(&["a", "b", "c"]);
    let g = s.geometry(960.0, 600.0);
    let l = s.home_layout(&g).unwrap();
    assert!(l.keys, "the wide form");
    assert!(l.heading.x > l.button(Button::New).right(), "two columns");
    let inside =
        |r: Rect| r.x >= 0.0 && r.y >= 0.0 && r.right() <= g.size.0 && r.bottom() <= g.size.1;
    for r in l
        .buttons
        .iter()
        .chain([&l.heading])
        .chain(l.search.iter())
        .chain(l.rows.iter().map(|(_, r)| r))
    {
        assert!(inside(*r), "{r:?} in {:?}", g.size);
    }
    assert!(!l.rows.is_empty());
}

/// The wheel over the list scrolls it, as on any list; elsewhere it does nothing
/// (UI-350 review).
#[test]
fn the_wheel_over_the_list_scrolls_it() {
    let names: Vec<String> = (0..12).map(|i| format!("s{i:02}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let (mut s, _, _tmp) = kept(&names);
    // Low enough that the 12 songs and the sample do not all fit.
    let g = s.geometry(960.0, 360.0);
    let l = s.home_layout(&g).unwrap();
    let max = home(&s).pick.max_offset(l.list_size);
    assert!(max > 0, "more rows than room");
    let list = l.list();
    let (x, y) = (list.x + list.w / 2.0, list.y + list.h / 2.0);
    let new = l.button(Button::New);
    assert!(
        !s.home_wheel(&g, new.x + 4.0, new.y + 4.0, -1.0),
        "not over the list"
    );
    assert_eq!(home(&s).pick.offset, 0);
    assert!(s.home_wheel(&g, x, y, -1.0));
    assert_eq!(home(&s).pick.offset, 1);
    for _ in 0..40 {
        s.home_wheel(&g, x, y, -1.0);
    }
    assert_eq!(home(&s).pick.offset, max, "kept to the end");
    assert!(s.home_wheel(&g, x, y, 1.0));
    assert_eq!(home(&s).pick.offset, max - 1);
    // The rows drawn follow the scroll.
    let l = s.home_layout(&g).unwrap();
    assert_eq!(l.rows[0].0, max - 1);
}

/// The start screen lies over an empty song: once that song gains anything (a drop, an
/// outside edit), the song shows, and undoing back to empty brings the new song's screen,
/// not the start screen (UI-350 review).
#[test]
fn the_start_screen_ends_when_the_song_under_it_changes() {
    let (mut s, g, _tmp) = kept(&["a"]);
    assert!(s.home_shown());
    assert!(s.run_command("track.add.audio", serde_json::json!({"name": "x"})));
    s.home_tick();
    assert!(s.home.is_none(), "the song shows");
    assert!(!s.start_shown());
    assert!(s.undo(&g));
    assert!(s.start_shown(), "empty again");
    s.home_tick();
    assert!(
        !s.home_shown(),
        "the new song's screen, not the start screen"
    );
    assert!(s.song.can_redo());
}

/// The window's own asks keep the keys over the start screen (UI-350 review).
#[test]
fn the_windows_asks_keep_the_keys_from_the_start_screen() {
    let (mut s, g, _tmp) = kept(&["a", "b"]);
    s.file_request = Some(FileRequest::Open);
    let at = home(&s).pick.selected;
    assert!(!s.home_key(&g, "down", NONE));
    assert!(!s.home_key(&g, "enter", NONE));
    assert_eq!(home(&s).pick.selected, at);
    s.file_request = None;
    assert!(s.home_key(&g, "down", NONE), "back once the dialog is gone");
}

/// The new song's screen tells a screen reader its entries, the missing input and the
/// line with no device (UI-350 review).
#[test]
fn a_screen_reader_reads_the_new_songs_entries() {
    let (mut s, g, _tmp) = kept(&["a"]);
    assert!(s.press(&g, Hit::Home(Spot::Button(Button::New))));
    s.wait_for_open_for_test();
    assert!(s.start_shown() && !s.home_shown());
    use accesskit::Role;
    let said = |s: &Studio| -> Vec<(Role, String, Option<String>)> {
        crate::a11y::start_marks(s, &g)
            .iter()
            .map(|m| {
                (
                    m.node.role(),
                    m.node.label().unwrap_or_default().to_string(),
                    m.node.description().map(str::to_string),
                )
            })
            .collect()
    };
    // A device with no inputs: the recording entry says so.
    assert_eq!(
        said(&s),
        [
            (
                Role::Button,
                "オーディオを録音".to_string(),
                Some(crate::input_row::NO_INPUT.to_string())
            ),
            (Role::Button, "MIDI を演奏".to_string(), None),
            (Role::Label, "ファイルをドロップ".to_string(), None),
        ]
    );
    let marks = crate::a11y::start_marks(&s, &g);
    assert_eq!(marks[0].press, Some(Hit::Start(crate::start::Entry::Audio)));
    // No device: the line once, and no mark on the entry.
    with_device(&mut s, false);
    let marks = said(&s);
    assert_eq!(marks[0].2, None);
    assert_eq!(
        marks.last().unwrap().1,
        crate::start::rig::NO_DEVICE.to_string()
    );
    // Not over the start screen.
    s.home = Some(Home::default());
    assert!(crate::a11y::start_marks(&s, &g).is_empty());
}

#[test]
fn with_no_device_the_line_under_the_buttons_opens_its_menu() {
    let (mut s, g, _tmp) = kept(&["a"]);
    with_device(&mut s, false);
    let l = s.home_layout(&g).unwrap();
    let line = l.no_device.expect("the line");
    assert!(line.y >= l.button(Button::Dawproject).bottom());
    assert_eq!(line.x, l.button(Button::New).x);
    let (x, y) = s.center_of(&g, Hit::Home(Spot::Device)).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Home(Spot::Device));
    assert!(s.press(&g, Hit::Home(Spot::Device)));
    let p = s.popup.as_ref().unwrap();
    assert_eq!(p.kind, crate::popup::PopupKind::NoDevice);
    let texts: Vec<_> = p
        .layout(g.size)
        .cells
        .iter()
        .map(|c| c.text.clone())
        .collect();
    assert_eq!(texts, ["再スキャン", "オーディオの設定…"]);
    // The menu has the keys and the letters while it is open.
    assert!(s.home_typing().is_none());
    assert!(!s.home_key(&g, "down", NONE));
}

#[test]
fn a_screen_reader_reads_the_buttons_the_search_and_the_list() {
    let (mut s, g, _tmp) = kept(&["a"]);
    with_device(&mut s, false);
    let marks = crate::a11y::marks(&s, &g);
    use accesskit::Role;
    let said: Vec<(Role, String)> = marks
        .iter()
        .map(|m| {
            (
                m.node.role(),
                m.node.label().unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            (Role::Button, "新規 Ctrl+N".to_string()),
            (Role::Button, "開く… Ctrl+O".to_string()),
            (Role::Button, "DAWproject を開く…".to_string()),
            (Role::Button, "オーディオデバイスなし".to_string()),
            (Role::TextInput, SEARCH_NAME.to_string()),
            (Role::List, RECENT.to_string()),
        ]
    );
    let list = marks.last().unwrap();
    let rows: Vec<(String, Option<bool>)> = list
        .children
        .iter()
        .map(|m| {
            (
                m.node.label().unwrap_or_default().to_string(),
                m.node.is_selected(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("a".to_string(), Some(true)),
            (
                format!("{} {SAMPLE_TAG}", crate::song::SAMPLE_NAME),
                Some(false)
            ),
        ]
    );
    // No transport under the start screen.
    assert!(crate::a11y::bottom_bar_nodes(&s, &g).is_empty());
}

fn finish_open(s: &mut Studio) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while s.opening.is_some() {
        assert!(std::time::Instant::now() < end, "open never returned");
        s.poll_opening();
        std::thread::yield_now();
    }
}

#[test]
fn ctrl_w_closes_the_song_and_the_start_screen_shows() {
    let mut s = Studio::sample();
    with_device(&mut s, true);
    let g = s.geometry(1280.0, 720.0);
    assert!(!s.home_shown());
    assert!(s.mapped_key(&g, "w", Some("w"), Mods::ctrl()));
    assert_eq!(s.window_ask, None, "Ctrl+W no longer quits");
    finish_open(&mut s);
    assert!(s.home_shown());
    assert!(s.project().root.is_empty());
    // On the start screen there is nothing to close.
    assert!(!s.close_to_home());
    assert!(s.opening.is_none());
}

#[test]
fn ctrl_f4_closes_the_song_like_ctrl_w() {
    let mut s = Studio::sample();
    with_device(&mut s, true);
    let g = s.geometry(1280.0, 720.0);
    assert!(s.mapped_key(&g, "f4", None, Mods::ctrl()));
    assert_eq!(
        s.window_ask, None,
        "Ctrl+F4 closes the document, it does not quit"
    );
    finish_open(&mut s);
    assert!(s.home_shown());
    assert!(s.project().root.is_empty());
}

#[test]
fn closing_from_the_song_menu_asks_first_when_the_plugin_state_was_not_kept() {
    let mut s = Studio::sample();
    with_device(&mut s, true);
    let g = s.geometry(1280.0, 720.0);
    s.keep.plugin_state_failed = true;
    let tracks = s.project().root.len();
    let popup = crate::popup::Popup::new(
        crate::popup::PopupKind::Song,
        Rect::new(40.0, -34.0, 160.0, 28.0),
    );
    s.popup = Some(popup);
    s.popup_action(&g, crate::popup::Action::CloseSong);
    s.poll_opening();
    assert!(s.unsaved.is_some());
    // Staying keeps the song.
    s.answer_unsaved(false);
    finish_open(&mut s);
    assert!(!s.home_shown());
    assert_eq!(s.project().root.len(), tracks);
    // Going on closes it.
    assert!(s.close_to_home());
    s.poll_opening();
    assert!(s.unsaved.is_some());
    s.answer_unsaved(true);
    finish_open(&mut s);
    assert!(s.home_shown());
    assert!(s.project().root.is_empty());
}
