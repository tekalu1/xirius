//! The drum view of the piano roll (EDT-006, EDT-058): the rows a drum part shows instead of the
//! keyboard, named by the sound they play (General MIDI), and which parts open in it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::id::ClipId;
use crate::model::{PluginKind, Project, Track, TrackIcon};
use crate::session::EditorView;

/// The General MIDI percussion names, from note 35.
const GM: [&str; 47] = [
    "バスドラム2",
    "キック",
    "サイドスティック",
    "スネア",
    "クラップ",
    "エレキスネア",
    "ロー・フロアタム",
    "ハイハット（閉）",
    "ハイ・フロアタム",
    "ハイハット（ペダル）",
    "ロータム",
    "ハイハット（開）",
    "ロー・ミッドタム",
    "ハイ・ミッドタム",
    "クラッシュ",
    "ハイタム",
    "ライド",
    "チャイニーズ",
    "ライドベル",
    "タンバリン",
    "スプラッシュ",
    "カウベル",
    "クラッシュ2",
    "ビブラスラップ",
    "ライド2",
    "ハイボンゴ",
    "ローボンゴ",
    "ミュート・ハイコンガ",
    "オープン・ハイコンガ",
    "ローコンガ",
    "ハイティンバレス",
    "ローティンバレス",
    "ハイアゴゴ",
    "ローアゴゴ",
    "カバサ",
    "マラカス",
    "ショートホイッスル",
    "ロングホイッスル",
    "ショートギロ",
    "ロングギロ",
    "クラベス",
    "ハイウッドブロック",
    "ローウッドブロック",
    "ミュート・クイーカ",
    "オープン・クイーカ",
    "ミュート・トライアングル",
    "オープン・トライアングル",
];

const FIRST: u8 = 35;

/// The name of a General MIDI percussion note.
pub fn name(pitch: u8) -> Option<&'static str> {
    pitch
        .checked_sub(FIRST)
        .and_then(|i| GM.get(usize::from(i)))
        .copied()
}

/// The maps a part can show: the name the command takes and the label the menu shows.
pub const MAPS: [(&str, &str); 2] = [("gm", "GM ドラム"), ("kit", "ドラムキット")];

/// The map a part shows when nothing was chosen.
pub const DEFAULT_MAP: &str = "gm";

pub fn map_label(map: &str) -> &'static str {
    MAPS.iter()
        .find(|(m, _)| *m == map)
        .map_or(MAPS[0].1, |(_, label)| label)
}

/// The kit's own notes (the ones the built-in Drum Kit plays).
const KIT: [u8; 34] = [
    35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 59,
    60, 61, 62, 63, 64, 69, 70, 75, 76, 77,
];

/// The everyday set: a kit as a player counts it.
const BASIC: [u8; 14] = [51, 49, 50, 48, 47, 45, 46, 44, 42, 39, 38, 37, 36, 41];

/// The rows of a map, the highest pitch first (the keyboard's order). An unknown map is the
/// default one.
pub fn rows(map: &str) -> Vec<(u8, &'static str)> {
    let mut pitches: Vec<u8> = match map {
        "kit" => KIT.to_vec(),
        _ => BASIC.to_vec(),
    };
    pitches.sort_unstable_by(|a, b| b.cmp(a));
    pitches
        .into_iter()
        .filter_map(|p| name(p).map(|n| (p, n)))
        .collect()
}

pub fn is_map(map: &str) -> bool {
    MAPS.iter().any(|(m, _)| *m == map)
}

/// Whether a track plays drums: it has the drums icon, or its instrument is a drum machine.
pub fn is_drum_track(track: &Track) -> bool {
    let hit = |s: &str| s.to_ascii_lowercase().contains("drum");
    track.icon == Some(TrackIcon::Drums)
        || track
            .inserts
            .iter()
            .any(|x| x.kind == PluginKind::Instrument && (hit(&x.name) || hit(&x.uid)))
}

/// The map a clip is shown in, or `None` for the keyboard. What was chosen for the part wins;
/// without a choice a part opens in the drum view when its instrument has a drum map.
pub fn shown(project: &Project, view: &EditorView, clip: ClipId) -> Option<String> {
    let (tid, _) = project.find_clip(clip)?;
    let track = project.tracks.get(&tid)?;
    let on = match view.drum_clips.get(&clip.to_string()) {
        Some(on) => *on,
        None => is_drum_track(track),
    };
    on.then(|| {
        view.drum_maps
            .get(&tid.to_string())
            .filter(|m| is_map(m))
            .cloned()
            .unwrap_or_else(|| DEFAULT_MAP.to_string())
    })
}

/// What was changed in a track's drum rows: their order, the rows hidden and their names. Empty
/// is the map as it comes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowEdits {
    /// The rows from the top, as last moved. A row not listed (one shown again, or one a new
    /// note brought) goes under the row above it in the map.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<u8>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<u8, String>,
    /// The names the track's instrument gives its keys (EDT-134). Read from the plugin each
    /// time, never kept with the song.
    #[serde(skip)]
    pub plugin: BTreeMap<u8, String>,
}

impl RowEdits {
    /// Nothing was changed by the user (the instrument's names are not a change).
    pub fn is_empty(&self) -> bool {
        self.order.is_empty() && self.hidden.is_empty() && self.names.is_empty()
    }

    /// The name a row shows: the one given, the instrument's, the General MIDI one, or the
    /// note's number.
    pub fn name(&self, pitch: u8) -> String {
        match self.names.get(&pitch).or_else(|| self.plugin.get(&pitch)) {
            Some(n) => n.clone(),
            None => name(pitch).map_or_else(|| format!("ノート {pitch}"), str::to_string),
        }
    }
}

/// Every row the map can show before any is hidden: its own and the ones `used` brings, the
/// highest pitch first.
pub fn all_rows(map: &str, used: impl IntoIterator<Item = u8>) -> Vec<u8> {
    let mut all: Vec<u8> = rows(map)
        .into_iter()
        .map(|(p, _)| p)
        .chain(used.into_iter().filter(|p| *p <= 127))
        .collect();
    all.sort_unstable_by(|a, b| b.cmp(a));
    all.dedup();
    all
}

/// The rows the grid shows, from the top: the map's and the ones `used` brings, without the
/// hidden ones (a row with notes on it always shows, so no note is hidden), in the order set.
pub fn shown_rows(map: &str, edits: &RowEdits, used: impl IntoIterator<Item = u8>) -> Vec<u8> {
    let used: Vec<u8> = used.into_iter().collect();
    let base: Vec<u8> = all_rows(map, used.iter().copied())
        .into_iter()
        .filter(|p| !edits.hidden.contains(p) || used.contains(p))
        .collect();
    let mut out: Vec<u8> = Vec::with_capacity(base.len());
    for p in &edits.order {
        if base.contains(p) && !out.contains(p) {
            out.push(*p);
        }
    }
    // The rest each go under the row above them in the map.
    let mut after: Option<usize> = None;
    for p in base {
        match out.iter().position(|q| *q == p) {
            Some(i) => after = Some(i),
            None => {
                let i = after.map_or(0, |i| i + 1);
                out.insert(i, p);
                after = Some(i);
            }
        }
    }
    out
}

/// The track's changes to its rows (none when nothing was changed).
pub fn edits<'a>(view: &'a EditorView, track: &str) -> std::borrow::Cow<'a, RowEdits> {
    match view.drum_rows.get(track) {
        Some(e) => std::borrow::Cow::Borrowed(e),
        None => std::borrow::Cow::Owned(RowEdits::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gm_names_run_from_35_to_81() {
        assert_eq!(name(35), Some("バスドラム2"));
        assert_eq!(name(36), Some("キック"));
        assert_eq!(name(38), Some("スネア"));
        assert_eq!(name(81), Some("オープン・トライアングル"));
        assert_eq!(name(34), None);
        assert_eq!(name(82), None);
    }

    #[test]
    fn a_map_lists_the_highest_pitch_first_and_names_every_row() {
        for (map, _) in MAPS {
            let rows = rows(map);
            assert!(rows.windows(2).all(|w| w[0].0 > w[1].0), "{map}");
            assert!(rows.iter().all(|(p, n)| name(*p) == Some(*n)));
        }
        assert_eq!(rows("gm").last(), Some(&(36, "キック")));
        assert_eq!(rows("kit").len(), 34);
        assert_eq!(rows("nothing"), rows("gm"));
    }

    #[test]
    fn hidden_rows_go_unless_they_have_notes_and_moved_rows_keep_their_place() {
        let gm = all_rows("gm", []);
        let none = RowEdits::default();
        assert_eq!(shown_rows("gm", &none, []), gm);
        // A note outside the map brings its row in pitch order.
        let with_90 = shown_rows("gm", &none, [90]);
        assert_eq!(with_90[0], 90);
        let hide = RowEdits {
            hidden: vec![42, 36],
            ..RowEdits::default()
        };
        let rows = shown_rows("gm", &hide, [36]);
        assert!(!rows.contains(&42));
        assert!(rows.contains(&36), "the kick has a note");
        // The kick moved to the top; the rest stay in the map's order.
        let moved = RowEdits {
            order: [36]
                .into_iter()
                .chain(gm.iter().copied().filter(|p| *p != 36))
                .collect(),
            ..RowEdits::default()
        };
        let rows = shown_rows("gm", &moved, []);
        assert_eq!(rows[0], 36);
        assert_eq!(&rows[1..], &gm[..gm.len() - 1]);
        // A row shown again goes back under the row above it in the map.
        let partial = RowEdits {
            order: gm.iter().copied().filter(|p| *p != 42).rev().collect(),
            ..RowEdits::default()
        };
        let rows = shown_rows("gm", &partial, []);
        let above = gm[gm.iter().position(|p| *p == 42).unwrap() - 1];
        let at = rows.iter().position(|p| *p == above).unwrap();
        assert_eq!(rows[at + 1], 42);
        assert_eq!(rows.len(), gm.len());
    }

    #[test]
    fn a_row_shows_the_name_given_else_the_gm_name_else_the_number() {
        let mut e = RowEdits::default();
        assert_eq!(e.name(36), "キック");
        assert_eq!(e.name(90), "ノート 90");
        e.names.insert(36, "808".into());
        assert_eq!(e.name(36), "808");
    }

    #[test]
    fn edt_134_the_instruments_name_comes_after_the_users_and_before_the_gm_one() {
        let mut e = RowEdits::default();
        e.plugin.insert(36, "Kick In".into());
        e.plugin.insert(90, "Ride Bell".into());
        assert_eq!(e.name(36), "Kick In");
        assert_eq!(e.name(90), "Ride Bell");
        assert_eq!(
            e.name(38),
            "スネア",
            "a key it does not name keeps the GM name"
        );
        assert_eq!(e.name(100), "ノート 100");
        assert!(
            e.is_empty(),
            "the instrument's names are no change of the user's"
        );
        e.names.insert(36, "808".into());
        assert_eq!(e.name(36), "808");
        assert!(!e.is_empty());
        // They are not kept with the song.
        let back: RowEdits = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert!(back.plugin.is_empty());
        assert_eq!(back.names, e.names);
    }
}
