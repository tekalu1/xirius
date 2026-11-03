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
    let side = Space::X6.px();
    let content = narrow.size.0 - 2.0 * side;
    assert!(content < NARROW);
    assert_eq!(l.heading.x, side);
    assert_eq!(l.heading.w, content, "the list at the full width");
    let lowest = l.buttons.iter().map(|r| r.bottom()).fold(0.0, f32::max);
    assert!(l.heading.y > lowest, "under the buttons");
    for b in l.buttons {
        assert!(b.right() <= side + content + 0.5, "{b:?}");
    }
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
            (Role::TextInput, "検索".to_string()),
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
