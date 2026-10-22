//! UI-066: the track list folds to a strip of the tracks' colours and opens again, by the ‹ / ›,
//! by a press on the strip and by Ctrl+Shift+L. Everything is pressed where the window sends it.

use xirius_design::Layout;

use crate::side_edge::{FOLDED_W, SideSize};
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

const W: f32 = 1440.0;
const H: f32 = 1000.0;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = false;
    let g = s.geometry(W, H);
    s.resized(&g);
    (s, g)
}

fn mid(r: crate::studio::Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// Ctrl+Shift+L, as the window sends it.
fn fold_key(s: &mut Studio) -> bool {
    let g = s.geometry(W, H);
    s.mapped_key(
        &g,
        "l",
        Some("L"),
        Mods {
            ctrl: true,
            shift: true,
            alt: false,
        },
    )
}

/// A press on a point of the window: what `screen.rs` does when the arrangement leaves it.
fn press_at(s: &mut Studio, (x, y): (f32, f32)) -> Hit {
    let g = s.geometry(W, H);
    let hit = s.hit(&g, x, y);
    assert!(
        !s.arrange_down(&g, x, y, Mods::NONE),
        "the arrangement took a press that is on the head: {hit:?}"
    );
    s.press(&g, hit);
    hit
}

#[test]
fn the_key_folds_the_list_to_a_strip_and_opens_it_again() {
    let (mut s, open) = studio();
    assert_eq!(open.column.w, s.tree().head_w.max(Layout::TRACK_HEAD_W));
    assert!(fold_key(&mut s), "Ctrl+Shift+L is bound");
    let folded = s.geometry(W, H);
    assert_eq!(folded.column.w, FOLDED_W);
    assert_eq!(folded.head.w, FOLDED_W);
    // What the strip frees goes to the arrangement; its rows keep where they are.
    assert_eq!(folded.lanes.right(), open.lanes.right());
    assert!(folded.lanes.x < open.lanes.x);
    assert_eq!(folded.lanes.w, open.lanes.w + (open.column.w - FOLDED_W));
    assert_eq!(
        (folded.section_band.h, folded.tempo.h, folded.ruler.h),
        (open.section_band.h, open.tempo.h, open.ruler.h)
    );
    assert!(fold_key(&mut s));
    assert_eq!(s.geometry(W, H).column.w, open.column.w);
}

#[test]
fn the_rows_keep_their_heights_when_the_list_is_folded() {
    let (mut s, g) = studio();
    let before: Vec<(f32, f32)> = s.tree().rows.iter().map(|r| (r.y, r.h)).collect();
    assert!(before.len() > 2);
    press_at(&mut s, mid(g.toggle(Hit::Fold).unwrap()));
    s.rebuild(&s.geometry(W, H));
    let after: Vec<(f32, f32)> = s.tree().rows.iter().map(|r| (r.y, r.h)).collect();
    assert_eq!(before, after);
}

#[test]
fn the_arrow_at_the_head_folds_and_the_strip_opens_it() {
    let (mut s, g) = studio();
    assert_eq!(
        s.hit(
            &g,
            mid(g.toggle(Hit::Fold).unwrap()).0,
            mid(g.toggle(Hit::Fold).unwrap()).1
        ),
        Hit::Fold
    );
    assert_eq!(
        press_at(&mut s, mid(g.toggle(Hit::Fold).unwrap())),
        Hit::Fold
    );
    let folded = s.geometry(W, H);
    assert_eq!(folded.column.w, FOLDED_W);
    // The one button of the folded head is the ›; the others are not there to press.
    for hit in Hit::HEAD {
        assert!(folded.toggle(hit).is_none(), "{hit:?}");
    }
    let arrow = mid(folded.toggle(Hit::Fold).unwrap());
    assert_eq!(s.hit(&folded, arrow.0, arrow.1), Hit::Fold);

    // A row of the strip: it names itself and a press opens the list.
    let row = s.tree().rows.first().expect("a row").kind;
    let top = s.row_y(&folded, s.tree().row_of(row).unwrap());
    let at = (folded.column.x + FOLDED_W / 2.0, top + 10.0);
    assert_eq!(s.hit(&folded, at.0, at.1), Hit::Folded(row));
    assert_eq!(press_at(&mut s, at), Hit::Folded(row));
    assert_eq!(s.geometry(W, H).column.w, g.column.w);
    // Folding it again and pressing the empty part of the strip below the rows opens it too.
    press_at(&mut s, mid(g.toggle(Hit::Fold).unwrap()));
    let folded = s.geometry(W, H);
    let below = (
        folded.column.x + FOLDED_W / 2.0,
        folded.column.bottom() - 2.0,
    );
    assert!(matches!(
        s.hit(&folded, below.0, below.1),
        Hit::Fold | Hit::Folded(_)
    ));
    press_at(&mut s, below);
    assert_eq!(s.geometry(W, H).column.w, g.column.w);
}

#[test]
fn l_still_locates_the_selection_and_does_not_fold() {
    let (mut s, _) = studio();
    let g = s.geometry(W, H);
    let _ = s.mapped_key(&g, "l", Some("l"), Mods::NONE);
    assert_eq!(s.geometry(W, H).column.w, studio().1.column.w);
    assert!(!s.side_size.folded);
}

#[test]
fn a_folded_strip_has_no_edge_to_drag() {
    let (mut s, _) = studio();
    assert!(fold_key(&mut s));
    let g = s.geometry(W, H);
    assert!(s.side_edge(&g, crate::side_edge::Side::Head).is_none());
}

#[test]
fn the_strip_names_a_row_and_the_arrow_says_what_it_does_now() {
    let (mut s, _) = studio();
    let open = crate::hover_tip::folded_text(&s, Hit::Fold).expect("the arrow's tip when open");
    assert!(open.starts_with("トラック一覧を畳む"), "{open}");
    assert!(open.contains("Ctrl+Shift+L"), "{open}");
    assert!(fold_key(&mut s));
    let project = s.project();
    let (id, track) = project.tracks.iter().next().expect("a track");
    assert_eq!(
        crate::hover_tip::folded_text(&s, Hit::Folded(RowKind::Track(*id))).as_deref(),
        Some(track.name.as_str())
    );
    let arrow = crate::hover_tip::folded_text(&s, Hit::Fold).expect("the arrow's tip");
    assert!(arrow.starts_with("トラック一覧を広げる"), "{arrow}");
    assert!(arrow.contains("Ctrl+Shift+L"), "{arrow}");
}

#[test]
fn the_fold_is_kept_for_the_next_window() {
    let dir = std::env::temp_dir().join(format!("xirius-tracklist-{}", std::process::id()));
    let file = dir.join("view.json");
    let (mut s, g) = studio();
    s.side_size.file = Some(file.clone());
    s.toggle_tracklist(&g);
    assert!(SideSize::load(&file).folded, "folded is written");
    let (mut again, g) = studio();
    again.side_size = SideSize::load(&file);
    assert_eq!(again.geometry(W, H).column.w, FOLDED_W);
    again.toggle_tracklist(&g);
    assert!(!SideSize::load(&file).folded, "opened is written");
    let _ = std::fs::remove_dir_all(dir);
    assert!(!SideSize::load(std::path::Path::new("no/such/view.json")).folded);
}
