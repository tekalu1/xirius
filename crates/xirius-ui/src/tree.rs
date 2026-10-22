//! The track column as geometry: which rows there are, where they sit, which
//! rounded block each belongs to, and where the folder range lines go.
//!
//! No GPUI here, so all of it is tested without a window.
//!
//! - MIX-037: a root folder with everything inside it is one rounded block; so
//!   is a run of consecutive root tracks. A lone track is a block of one row,
//!   rounded at the top and the bottom. Blocks are told apart by
//!   `Layout::BLOCK_GAP`, and rows inside a block are flat: only the first row
//!   of a block has round top corners and only the last has round bottom ones
//!   ([`Row::round_top`] / [`Row::round_bottom`]), so no corner is ever square.
//! - MIX-038: every open folder with something in it gets a [`Bar`], a
//!   vertical range line at `StructureLine::gutter_x(depth)`, stopped
//!   `StructureLine::END_INSET` inside its first and last row. A collapsed
//!   folder has no bar. Folders past the indent cap draw theirs on the
//!   deepest gutter, over their parent's, as the mock does.
//! - The head is as wide as the deepest indent in the song needs
//!   ([`Layout::track_head_w`]), so the buttons of a deeply nested track stay
//!   inside it.

use std::collections::{HashMap, HashSet};

use xirius_core::id::{FolderId, Node, TrackId};
use xirius_core::model::Project;
use xirius_design::{Layout, Space, StructureLine};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Track(TrackId),
    Folder(FolderId),
}

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub kind: RowKind,
    /// 0 at the root; the contents of a root folder are 1.
    pub depth: usize,
    /// The folder this row sits directly in (None at the root).
    pub parent: Option<FolderId>,
    pub block: usize,
    /// Top, relative to the top of the track column.
    pub y: f32,
    pub h: f32,
    pub round_top: bool,
    pub round_bottom: bool,
    /// A track widened vertically (it shows its output).
    pub tall: bool,
    /// Automation lanes shown under a track, each [`LANE_H`] high (AUT-001).
    pub lanes: usize,
    /// The recording area under the lanes: the input of an armed track and the takes of what it
    /// recorded (REC-002, REC-005). 0 when there is none.
    pub rec_h: f32,
    /// The tracks a running bounce is making below this one, shown before they are made
    /// (EXP-047). 0 when there are none.
    pub job_h: f32,
    /// The width of the track head this row sits in ([`Tree::head_w`]).
    pub head_w: f32,
}

/// The height of one automation lane under its track.
pub const LANE_H: f32 = 56.0;

impl Row {
    pub fn bottom(&self) -> f32 {
        self.y + self.full_h()
    }

    /// The row with its automation lanes.
    pub fn full_h(&self) -> f32 {
        self.h + self.lanes as f32 * LANE_H + self.rec_h + self.job_h
    }

    /// Top of the tracks a bounce is making, relative to the top of the track column.
    pub fn job_y(&self) -> f32 {
        self.rec_y() + self.rec_h
    }

    /// Top of the recording area, relative to the top of the track column.
    pub fn rec_y(&self) -> f32 {
        self.y + self.h + self.lanes as f32 * LANE_H
    }

    /// Top of lane `i`, relative to the top of the track column.
    pub fn lane_y(&self, i: usize) -> f32 {
        self.y + self.h + i as f32 * LANE_H
    }

    /// Where the row's own content starts, from the block's left edge.
    pub fn content_x(&self) -> f32 {
        Space::X2.px() + self.depth.min(Layout::INDENT_MAX_DEPTH) as f32 * Layout::INDENT
    }

    /// How many levels deeper than the indent goes: shown as dots, not as space.
    pub fn depth_dots(&self) -> usize {
        self.depth.saturating_sub(Layout::INDENT_MAX_DEPTH)
    }

    /// The folder the pointer is "in" while it is over this row: the folder
    /// itself for a folder row, the enclosing one for a track.
    pub fn folder_in_scope(&self) -> Option<FolderId> {
        match self.kind {
            RowKind::Folder(f) => Some(f),
            RowKind::Track(_) => self.parent,
        }
    }
}

/// One rounded container.
#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub y: f32,
    pub h: f32,
    /// Index range of its rows (`first..=last`).
    pub first: usize,
    pub last: usize,
}

impl Block {
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// A folder's range line.
#[derive(Clone, Copy, Debug)]
pub struct Bar {
    pub folder: FolderId,
    /// The folder's own depth (0 for a root folder).
    pub depth: usize,
    /// From the block's left edge.
    pub x: f32,
    pub y0: f32,
    pub y1: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub rows: Vec<Row>,
    pub blocks: Vec<Block>,
    pub bars: Vec<Bar>,
    /// Bottom of the last block.
    pub height: f32,
    /// The track head's width: wider when the song nests deeper than
    /// `Layout::HEAD_ROOM_DEPTH`, collapsed folders included, so folding
    /// does not move the arrangement sideways.
    pub head_w: f32,
}

impl Tree {
    pub fn build(project: &Project, collapsed: &HashSet<FolderId>) -> Tree {
        Tree::build_with(project, collapsed, &HashSet::new())
    }

    /// Like [`Tree::build`], with the tracks in `tall` widened vertically.
    pub fn build_with(
        project: &Project,
        collapsed: &HashSet<FolderId>,
        tall: &HashSet<TrackId>,
    ) -> Tree {
        Tree::build_shaped(
            project,
            collapsed,
            tall,
            Layout::TRACK_ROW_H,
            &HashMap::new(),
        )
    }

    /// Like [`Tree::build_with`], with every track `track_h` high (the
    /// vertical zoom) and `lanes` automation lanes under the tracks listed.
    pub fn build_shaped(
        project: &Project,
        collapsed: &HashSet<FolderId>,
        tall: &HashSet<TrackId>,
        track_h: f32,
        lanes: &HashMap<TrackId, usize>,
    ) -> Tree {
        Tree::build_rec(project, collapsed, tall, track_h, lanes, &HashMap::new())
    }

    /// Like [`Tree::build_rec`], with `job` high areas under the tracks a running bounce
    /// puts its new tracks below.
    pub fn build_job(
        project: &Project,
        collapsed: &HashSet<FolderId>,
        tall: &HashSet<TrackId>,
        track_h: f32,
        lanes: &HashMap<TrackId, usize>,
        rec: &HashMap<TrackId, f32>,
        job: &HashMap<TrackId, f32>,
    ) -> Tree {
        Tree::build_all(project, collapsed, tall, track_h, lanes, rec, job)
    }

    /// Like [`Tree::build_shaped`], with `rec` high recording areas under the tracks listed.
    pub fn build_rec(
        project: &Project,
        collapsed: &HashSet<FolderId>,
        tall: &HashSet<TrackId>,
        track_h: f32,
        lanes: &HashMap<TrackId, usize>,
        rec: &HashMap<TrackId, f32>,
    ) -> Tree {
        Tree::build_all(
            project,
            collapsed,
            tall,
            track_h,
            lanes,
            rec,
            &HashMap::new(),
        )
    }

    fn build_all(
        project: &Project,
        collapsed: &HashSet<FolderId>,
        tall: &HashSet<TrackId>,
        track_h: f32,
        lanes: &HashMap<TrackId, usize>,
        rec: &HashMap<TrackId, f32>,
        job: &HashMap<TrackId, f32>,
    ) -> Tree {
        let mut b = Builder {
            head_w: Layout::track_head_w(deepest(project)),
            project,
            collapsed,
            tall,
            track_h,
            lanes,
            rec,
            job,
            rows: Vec::new(),
            blocks: Vec::new(),
            bars: Vec::new(),
            y: 0.0,
            open_run: false,
            spans: Vec::new(),
        };
        for node in &project.root {
            match *node {
                Node::Track(t) => {
                    if !b.open_run {
                        b.begin_block();
                        b.open_run = true;
                    }
                    b.push_track(t, 0, None);
                }
                Node::Folder(f) => {
                    b.open_run = false;
                    b.begin_block();
                    b.push_folder(f, 0, None);
                }
            }
        }
        b.finish()
    }

    pub fn row_at(&self, y: f32) -> Option<&Row> {
        self.rows.iter().find(|r| y >= r.y && y < r.bottom())
    }

    pub fn row_of(&self, kind: RowKind) -> Option<&Row> {
        self.rows.iter().find(|r| r.kind == kind)
    }

    pub fn block_of(&self, row: &Row) -> &Block {
        &self.blocks[row.block]
    }
}

/// The depth of the deepest row in the song, open or not.
fn deepest(project: &Project) -> usize {
    fn walk(project: &Project, nodes: &[Node], depth: usize) -> usize {
        nodes
            .iter()
            .map(|n| match n {
                Node::Track(_) => depth,
                Node::Folder(f) => project
                    .folders
                    .get(f)
                    .map_or(depth, |f| walk(project, &f.children, depth + 1).max(depth)),
            })
            .max()
            .unwrap_or(0)
    }
    walk(project, &project.root, 0)
}

struct Builder<'a> {
    head_w: f32,
    project: &'a Project,
    collapsed: &'a HashSet<FolderId>,
    tall: &'a HashSet<TrackId>,
    track_h: f32,
    lanes: &'a HashMap<TrackId, usize>,
    rec: &'a HashMap<TrackId, f32>,
    job: &'a HashMap<TrackId, f32>,
    rows: Vec<Row>,
    blocks: Vec<Block>,
    bars: Vec<Bar>,
    y: f32,
    open_run: bool,
    /// (folder, depth, first row, last row) of every open folder with contents.
    spans: Vec<(FolderId, usize, usize, usize)>,
}

impl Builder<'_> {
    fn begin_block(&mut self) {
        if !self.blocks.is_empty() {
            self.y += Layout::BLOCK_GAP;
        }
        self.blocks.push(Block {
            y: self.y,
            h: 0.0,
            first: self.rows.len(),
            last: self.rows.len(),
        });
    }

    fn push_row(
        &mut self,
        kind: RowKind,
        depth: usize,
        parent: Option<FolderId>,
        h: f32,
        tall: bool,
    ) -> usize {
        let block = self.blocks.len() - 1;
        let lanes = match kind {
            RowKind::Track(t) => self.lanes.get(&t).copied().unwrap_or(0),
            RowKind::Folder(_) => 0,
        };
        let rec_h = match kind {
            RowKind::Track(t) => self.rec.get(&t).copied().unwrap_or(0.0),
            RowKind::Folder(_) => 0.0,
        };
        let job_h = match kind {
            RowKind::Track(t) => self.job.get(&t).copied().unwrap_or(0.0),
            RowKind::Folder(_) => 0.0,
        };
        self.rows.push(Row {
            kind,
            depth,
            parent,
            block,
            y: self.y,
            h,
            round_top: false,
            round_bottom: false,
            tall,
            lanes,
            rec_h,
            job_h,
            head_w: self.head_w,
        });
        self.y += h + lanes as f32 * LANE_H + rec_h + job_h;
        self.rows.len() - 1
    }

    fn push_track(&mut self, id: TrackId, depth: usize, parent: Option<FolderId>) {
        let widened = self.tall.contains(&id);
        let h = if widened {
            self.track_h
                .max(Layout::TRACK_ROW_TALL_H + crate::fx_chain::HEADER_H)
        } else {
            self.track_h
        };
        let tall = widened || h >= Layout::TRACK_ROW_TALL_H;
        self.push_row(RowKind::Track(id), depth, parent, h, tall);
    }

    fn push_folder(&mut self, id: FolderId, depth: usize, parent: Option<FolderId>) {
        self.push_row(
            RowKind::Folder(id),
            depth,
            parent,
            Layout::FOLDER_ROW_H,
            false,
        );
        if self.collapsed.contains(&id) {
            return;
        }
        let Some(folder) = self.project.folders.get(&id) else {
            return;
        };
        let first = self.rows.len();
        for child in &folder.children {
            match *child {
                Node::Track(t) => self.push_track(t, depth + 1, Some(id)),
                Node::Folder(f) => self.push_folder(f, depth + 1, Some(id)),
            }
        }
        if self.rows.len() > first {
            self.spans.push((id, depth, first, self.rows.len() - 1));
        }
    }

    fn finish(mut self) -> Tree {
        for i in 0..self.blocks.len() {
            let first = self.blocks[i].first;
            let last = self
                .blocks
                .get(i + 1)
                .map_or(self.rows.len(), |next| next.first)
                - 1;
            let bottom = self.rows[last].bottom();
            let block = &mut self.blocks[i];
            block.last = last;
            block.h = bottom - block.y;
            self.rows[first].round_top = true;
            self.rows[last].round_bottom = true;
        }
        for &(folder, depth, first, last) in &self.spans {
            // Past the indent cap the children do not step in, so there is no
            // gutter of its own: the line goes on the deepest one.
            let gutter = depth.min(Layout::INDENT_MAX_DEPTH - 1);
            self.bars.push(Bar {
                folder,
                depth,
                x: StructureLine::gutter_x(gutter),
                y0: self.rows[first].y + StructureLine::END_INSET,
                y1: self.rows[last].bottom() - StructureLine::END_INSET,
            });
        }
        // Outer folders first, so an inner bar is drawn on top.
        self.bars.sort_by_key(|b| (b.depth, b.folder));
        Tree {
            height: self.blocks.last().map_or(0.0, Block::bottom),
            rows: self.rows,
            blocks: self.blocks,
            bars: self.bars,
            head_w: self.head_w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::Song;

    fn tree(collapsed: &[&str]) -> (Tree, std::sync::Arc<Project>) {
        let p = Song::sample().project();
        let set = p
            .folders
            .values()
            .filter(|f| collapsed.contains(&f.name.as_str()))
            .map(|f| f.id)
            .collect();
        (Tree::build(&p, &set), p)
    }

    fn name(p: &Project, row: &Row) -> String {
        match row.kind {
            RowKind::Track(t) => p.tracks[&t].name.clone(),
            RowKind::Folder(f) => p.folders[&f].name.clone(),
        }
    }

    fn folder_id(p: &Project, name: &str) -> FolderId {
        p.folders.values().find(|f| f.name == name).unwrap().id
    }

    #[test]
    fn a_widened_track_is_taller_and_the_rows_below_move_down() {
        let p = Song::sample().project();
        let (plain, _) = tree(&[]);
        let first = plain
            .rows
            .iter()
            .find_map(|r| match r.kind {
                RowKind::Track(t) => Some(t),
                RowKind::Folder(_) => None,
            })
            .unwrap();
        let wide = Tree::build_with(&p, &HashSet::new(), &HashSet::from([first]));
        let before = plain.row_of(RowKind::Track(first)).unwrap();
        let after = wide.row_of(RowKind::Track(first)).unwrap();
        assert!(after.tall && !before.tall);
        assert_eq!(
            after.h,
            Layout::TRACK_ROW_TALL_H + crate::fx_chain::HEADER_H
        );
        assert_eq!(
            wide.height - plain.height,
            Layout::TRACK_ROW_TALL_H + crate::fx_chain::HEADER_H - before.h
        );
    }

    #[test]
    fn lanes_hang_under_their_track_and_the_zoom_sets_every_track_height() {
        let p = Song::sample().project();
        let (plain, _) = tree(&[]);
        let keys = plain
            .rows
            .iter()
            .find(|r| name(&p, r) == "Keys")
            .map(|r| r.kind)
            .unwrap();
        let RowKind::Track(keys_id) = keys else {
            panic!("Keys is a track")
        };
        let lanes = HashMap::from([(keys_id, 2)]);
        let t = Tree::build_shaped(&p, &HashSet::new(), &HashSet::new(), 80.0, &lanes);
        let row = t.row_of(keys).unwrap();
        assert_eq!((row.h, row.lanes), (80.0, 2));
        assert_eq!(row.lane_y(1), row.y + 80.0 + LANE_H);
        // The lane area belongs to its track; the next row starts below the lanes.
        assert_eq!(t.row_at(row.lane_y(1) + 1.0).unwrap().kind, keys);
        let next = &t.rows[index(&t, row) + 1];
        assert_eq!(next.y, row.bottom());
        // Folder rows keep their height.
        assert_eq!(t.rows[0].h, Layout::FOLDER_ROW_H);
        assert!(
            t.rows
                .iter()
                .all(|r| matches!(r.kind, RowKind::Folder(_)) || r.h == 80.0)
        );
    }

    #[test]
    fn rows_come_in_column_order_with_their_depth() {
        let (t, p) = tree(&[]);
        let got: Vec<_> = t.rows.iter().map(|r| (name(&p, r), r.depth)).collect();
        let want = [
            ("Rhythm", 0),
            ("Drums", 1),
            ("Bass", 1),
            ("Percussion", 1),
            ("Shaker", 2),
            ("Latin", 2),
            ("Conga", 3),
            ("Keys", 0),
            ("Vocal", 0),
            ("Guitar", 0),
        ];
        let want: Vec<_> = want.iter().map(|(n, d)| (n.to_string(), *d)).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn a_folder_with_everything_in_it_is_one_block_and_the_loose_tracks_another() {
        let (t, p) = tree(&[]);
        assert_eq!(t.blocks.len(), 2);
        assert_eq!(t.blocks[0].first, 0);
        assert_eq!(name(&p, &t.rows[t.blocks[0].last]), "Conga");
        assert_eq!(name(&p, &t.rows[t.blocks[1].first]), "Keys");
        assert_eq!(name(&p, &t.rows[t.blocks[1].last]), "Guitar");
        assert!(
            t.rows
                .iter()
                .all(|r| t.blocks[r.block].first <= index(&t, r))
        );
    }

    fn index(t: &Tree, r: &Row) -> usize {
        t.rows.iter().position(|x| x.kind == r.kind).unwrap()
    }

    #[test]
    fn blocks_are_a_gap_apart_and_rows_inside_a_block_touch() {
        let (t, _) = tree(&[]);
        let gap = t.blocks[1].y - t.blocks[0].bottom();
        assert_eq!(gap, Layout::BLOCK_GAP);
        for b in &t.blocks {
            let rows = &t.rows[b.first..=b.last];
            assert_eq!(rows[0].y, b.y);
            for pair in rows.windows(2) {
                assert_eq!(pair[0].bottom(), pair[1].y);
            }
            assert_eq!(rows.last().unwrap().bottom(), b.bottom());
            assert_eq!(b.h, rows.iter().map(|r| r.h).sum::<f32>());
        }
        assert_eq!(t.height, t.blocks[1].bottom());
    }

    #[test]
    fn only_the_ends_of_a_block_are_round() {
        let (t, _) = tree(&[]);
        for (i, r) in t.rows.iter().enumerate() {
            let b = &t.blocks[r.block];
            assert_eq!(r.round_top, i == b.first, "row {i}");
            assert_eq!(r.round_bottom, i == b.last, "row {i}");
        }
    }

    #[test]
    fn a_lone_track_is_round_at_the_top_and_the_bottom() {
        let mut p = (*Song::sample().project()).clone();
        // Keep only Keys of the loose tracks, so the last block is one row.
        let keep = p
            .root
            .iter()
            .position(|n| matches!(n, Node::Track(_)))
            .unwrap();
        p.root.truncate(keep + 1);
        let t = Tree::build(&p, &HashSet::new());
        let last = t.blocks.last().unwrap();
        assert_eq!(last.first, last.last);
        let row = &t.rows[last.first];
        assert!(row.round_top && row.round_bottom);
    }

    #[test]
    fn a_track_between_two_folders_gets_its_own_block() {
        let mut p = (*Song::sample().project()).clone();
        let f = p.folders.keys().next().copied().unwrap();
        // Folder, track, track, folder, track.
        let tracks: Vec<_> = p
            .root
            .iter()
            .filter(|n| matches!(n, Node::Track(_)))
            .copied()
            .collect();
        p.root = vec![
            Node::Folder(f),
            tracks[0],
            tracks[1],
            Node::Folder(folder_id(&p, "Latin")),
            tracks[2],
        ];
        let t = Tree::build(&p, &HashSet::new());
        let sizes: Vec<_> = t.blocks.iter().map(|b| b.last - b.first + 1).collect();
        // Rhythm has 7 rows with Latin open inside it; Latin at the root has 2.
        assert_eq!(sizes.len(), 4);
        assert_eq!(sizes[1], 2);
        assert_eq!(sizes[3], 1);
    }

    #[test]
    fn three_nested_folders_have_three_lines_side_by_side() {
        let (t, p) = tree(&[]);
        let by_name: Vec<_> = t
            .bars
            .iter()
            .map(|b| (p.folders[&b.folder].name.clone(), b.depth, b.x))
            .collect();
        assert_eq!(
            by_name,
            vec![
                ("Rhythm".to_string(), 0, StructureLine::gutter_x(0)),
                ("Percussion".to_string(), 1, StructureLine::gutter_x(1)),
                ("Latin".to_string(), 2, StructureLine::gutter_x(2)),
            ]
        );
        // Conga, three folders deep, steps in past Latin's line, and has no dot.
        let latin = t.row_of(RowKind::Folder(folder_id(&p, "Latin"))).unwrap();
        let conga = t
            .rows
            .iter()
            .find(|r| r.parent == Some(folder_id(&p, "Latin")))
            .expect("a row inside Latin");
        assert_eq!(conga.depth, 3);
        assert_eq!(conga.depth_dots(), 0);
        assert_eq!(conga.content_x(), latin.content_x() + Layout::INDENT);
        // Each line sits left of the content of the rows it spans.
        for b in &t.bars {
            let first_inside = t
                .rows
                .iter()
                .find(|r| r.parent == Some(b.folder))
                .expect("a row inside");
            assert!(b.x + StructureLine::THICKNESS <= first_inside.content_x());
        }
    }

    /// The sample song with `levels` more folders between Latin and Conga.
    fn nested_deeper(levels: u64) -> Project {
        let mut p = (*Song::sample().project()).clone();
        let latin = folder_id(&p, "Latin");
        let mut parent = latin;
        for i in 0..levels {
            let id = FolderId(9000 + i);
            let children = std::mem::take(&mut p.folders.get_mut(&parent).unwrap().children);
            p.folders.insert(
                id,
                xirius_core::model::Folder {
                    id,
                    name: format!("Deep {i}"),
                    color: None,
                    bus: None,
                    children,
                },
            );
            p.folders.get_mut(&parent).unwrap().children = vec![Node::Folder(id)];
            parent = id;
        }
        p
    }

    #[test]
    fn past_the_indent_cap_rows_show_dots_and_lines_share_the_deepest_gutter() {
        let p = nested_deeper(2);
        let t = Tree::build(&p, &HashSet::new());
        let conga = t.rows.iter().find(|r| name(&p, r) == "Conga").unwrap();
        assert_eq!(conga.depth, 5);
        assert_eq!(conga.depth_dots(), 2);
        let deep_1 = t.rows.iter().find(|r| name(&p, r) == "Deep 1").unwrap();
        assert_eq!(conga.content_x(), deep_1.content_x());
        let deepest_gutter = StructureLine::gutter_x(Layout::INDENT_MAX_DEPTH - 1);
        for b in t
            .bars
            .iter()
            .filter(|b| b.depth >= Layout::INDENT_MAX_DEPTH - 1)
        {
            assert_eq!(b.x, deepest_gutter);
        }
        // The dots sit at the head's left edge, centred on the row, left of
        // every folder's line.
        let dot = crate::studio::RowParts::depth_dot(conga, 0);
        let last = crate::studio::RowParts::depth_dot(conga, 1);
        assert!(
            ((dot.y + last.bottom()) / 2.0 - conga.h / 2.0).abs() < 0.01,
            "{dot:?} {last:?}"
        );
        assert!(dot.x + dot.w < StructureLine::gutter_x(0), "{dot:?}");
    }

    #[test]
    fn the_head_widens_for_a_third_level_and_keeps_its_width_when_folded() {
        let (open, _) = tree(&[]);
        assert_eq!(open.head_w, Layout::TRACK_HEAD_W + Layout::INDENT);
        assert!(open.rows.iter().all(|r| r.head_w == open.head_w));
        let (folded, _) = tree(&["Latin"]);
        assert_eq!(folded.head_w, open.head_w);
        let mut p = (*Song::sample().project()).clone();
        p.root.retain(|n| matches!(n, Node::Track(_)));
        let flat = Tree::build(&p, &HashSet::new());
        assert_eq!(flat.head_w, Layout::TRACK_HEAD_W);
    }

    #[test]
    fn range_lines_stop_inside_the_ends_of_their_rows_and_never_reach_a_block_corner() {
        let (t, p) = tree(&[]);
        let inset = StructureLine::END_INSET;
        for b in &t.bars {
            let inside: Vec<_> = t
                .rows
                .iter()
                .filter(|r| is_inside(&t, &p, r, b.folder))
                .collect();
            let top = inside.first().unwrap();
            let bottom = inside.last().unwrap();
            assert_eq!(b.y0, top.y + inset);
            assert_eq!(b.y1, bottom.bottom() - inset);
            assert!(b.y1 > b.y0);
            let block = t.block_of(top);
            assert!(b.y0 >= block.y + inset && b.y1 <= block.bottom() - inset);
        }
    }

    /// Whether `row` is somewhere below `folder`.
    fn is_inside(t: &Tree, p: &Project, row: &Row, folder: FolderId) -> bool {
        let mut at = row.parent;
        while let Some(f) = at {
            if f == folder {
                return true;
            }
            at = t.row_of(RowKind::Folder(f)).and_then(|r| r.parent);
        }
        let _ = p;
        false
    }

    #[test]
    fn a_collapsed_folder_hides_its_rows_and_has_no_line() {
        let (t, p) = tree(&["Latin"]);
        assert!(t.rows.iter().all(|r| name(&p, r) != "Conga"));
        let latin = folder_id(&p, "Latin");
        assert!(t.bars.iter().all(|b| b.folder != latin));
        assert!(t.row_of(RowKind::Folder(latin)).is_some());

        let (t, p) = tree(&["Rhythm"]);
        assert_eq!(t.rows.len(), 1 + 3);
        assert!(t.bars.is_empty());
        let rhythm = &t.rows[0];
        // The collapsed block is just the folder row, round at both ends.
        assert!(rhythm.round_top && rhythm.round_bottom);
        assert_eq!(name(&p, rhythm), "Rhythm");
    }

    #[test]
    fn collapsing_shortens_the_column() {
        let (open, _) = tree(&[]);
        let (shut, _) = tree(&["Rhythm"]);
        assert!(shut.height < open.height);
        let drop = open.height - shut.height;
        let hidden: f32 = open.rows[1..7].iter().map(|r| r.h).sum();
        assert_eq!(drop, hidden);
    }

    #[test]
    fn rows_are_found_by_height() {
        let (t, p) = tree(&[]);
        let r = t.row_at(Layout::FOLDER_ROW_H + 1.0).unwrap();
        assert_eq!(name(&p, r), "Drums");
        assert!(t.row_at(t.height + 1.0).is_none());
        // In the gap between two blocks there is no row.
        let gap_y = t.blocks[0].bottom() + Layout::BLOCK_GAP / 2.0;
        assert!(t.row_at(gap_y).is_none());
    }

    #[test]
    fn the_scope_of_a_row_is_its_folder() {
        let (t, p) = tree(&[]);
        let shaker = t.rows.iter().find(|r| name(&p, r) == "Shaker").unwrap();
        assert_eq!(shaker.folder_in_scope(), Some(folder_id(&p, "Percussion")));
        let perc = t.rows.iter().find(|r| name(&p, r) == "Percussion").unwrap();
        assert_eq!(perc.folder_in_scope(), Some(folder_id(&p, "Percussion")));
        let keys = t.rows.iter().find(|r| name(&p, r) == "Keys").unwrap();
        assert_eq!(keys.folder_in_scope(), None);
    }

    #[test]
    fn an_empty_song_is_an_empty_column() {
        let t = Tree::build(&Project::new("x"), &HashSet::new());
        assert!(t.rows.is_empty() && t.blocks.is_empty());
        assert_eq!(t.height, 0.0);
    }
}
