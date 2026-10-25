//! UI-004: the same operation is not on the screen in two places. This is the inventory: every
//! thing the pointer can press (a `Hit`, a cell of a pop-over) is named by the operation it does,
//! and the test reads the whole screen (a scan of what is under the pointer, in each state of the
//! drawer and the browser, and every pop-over that opens) to find an operation that is reached
//! from two places. The few the approved mocks draw twice are listed below with their ground; any
//! other one fails. `docs/operations.md` is the list as the scan found it.
//!
//! Both mappings match every variant, so a new button cannot be added without being named here.

use crate::browser::BrowserHit;
use crate::mixer::MixerHit;
use crate::popup::{Action, PopupKind};
use crate::rec_panel::RecPart;
use crate::start::{Entry, Part};
use crate::studio::Hit;
use crate::topbar::BarButton;

/// Where an operation is pressed, and what it does. `None` for a press that does nothing by itself
/// (the empty part of a pop-over, a row that only selects is still an operation).
pub fn of_hit(hit: Hit) -> Option<(&'static str, &'static str)> {
    let m = match hit {
        Hit::None | Hit::PopupBody | Hit::Item(_) => return None,
        // The toast takes its presses before the rest of the window.
        Hit::Toast(_) => return None,
        // The kind icon only answers a crashed plugin's bolt.
        Hit::TrackIcon(_) => return None,
        // The mark says its values on hover; a press is the row's.
        Hit::TrackMark(_) => return None,
        Hit::MidiCancel => ("midi.learn", "midimap.learn"),
        Hit::Chevron(_) => ("fold", "folder.fold"),
        Hit::Mute(_) => ("head.mute", "track.mute"),
        Hit::Solo(_) => ("head.solo", "track.solo"),
        Hit::Row(_) => ("row", "row.select"),
        Hit::Clip(..) => ("clip", "clip.select"),
        Hit::Ruler(_) => ("ruler", "transport.seek"),
        Hit::Tab(_) => ("drawer.tab", "drawer.tab"),
        Hit::RollView(_) => ("drawer.roll.view", "roll.view"),
        Hit::RollMap => ("drawer.roll.map", "roll.view"),
        Hit::RollLength(_) => ("drawer.roll.length", "prefs.set"),
        Hit::Arm(_) => ("head.arm", "track.arm"),
        Hit::Monitor(_) => ("head.monitor", "track.monitor"),
        Hit::Keys(_) => ("head.keys", "instrument.open"),
        Hit::TrackColor(_) => ("head.color", "track.color.menu"),
        Hit::Widen(_) => ("head.widen", "track.widen"),
        Hit::Freeze(_) => ("head.freeze", "track.freeze"),
        Hit::Output(_) => ("head.output", "track.output.menu"),
        Hit::FolderBus(_) => ("head.bus", "folder.bus.menu"),
        Hit::Rec(part) => match part {
            RecPart::Punch => ("rec.punch", "record.punch"),
            RecPart::PreRoll => ("rec.preroll", "record.preroll"),
            RecPart::PreRollBars => ("rec.preroll_bars", "record.preroll.bars"),
            RecPart::PostRoll => ("rec.postroll", "record.postroll"),
            RecPart::PostRollBars => ("rec.postroll_bars", "record.postroll.bars"),
            RecPart::Count => ("rec.count", "record.count-in"),
            RecPart::CountBars => ("rec.count_bars", "record.count-in.bars"),
            RecPart::Quantize => ("rec.quantize", "record.input-quantize"),
            RecPart::QuantizeValue => ("rec.quantize_value", "record.input-quantize.menu"),
        },
        Hit::ToggleArrange => ("head.arrange", "view.arrange.toggle"),
        Hit::ToggleTempo => ("head.tempo", "view.tempo.toggle"),
        Hit::AddTrack => ("head.add", "track.add.menu"),
        Hit::ZoomOut => ("head.zoom_out", "view.zoom.out"),
        Hit::ZoomIn => ("head.zoom_in", "view.zoom.in"),
        Hit::WaveZoom => ("head.wave_zoom", "view.zoom.wave"),
        Hit::Follow => ("head.follow", "view.follow"),
        Hit::Fold => ("head.fold", "view.tracklist.collapse"),
        // The strip is one place, opened by a press anywhere on it.
        Hit::Folded(_) => ("tracklist.strip", "tracklist.strip.open"),
        Hit::Rail(_) => ("rail.tab", "browser.tab"),
        Hit::RailSettings => ("rail.settings", "settings.open"),
        Hit::Inspector(crate::inspector::Part::Field(_)) => ("inspector.field", "inspector.value"),
        Hit::Browser(h) => match h {
            BrowserHit::Search => ("browser.search", "browser.search"),
            BrowserHit::ViewButton => ("browser.view", "browser.view.menu"),
            BrowserHit::Group(_) => ("browser.group", "browser.group.fold"),
            BrowserHit::Item(_) => ("browser.item", "browser.item.pick"),
            BrowserHit::Fav(_) => ("browser.fav", "browser.fav.toggle"),
            BrowserHit::Dir(_) => ("browser.dir", "browser.dir.open"),
            BrowserHit::File(_) => ("browser.file", "browser.file.pick"),
            // The count is a part of its row, whose press it does.
            BrowserHit::Pool(_) | BrowserHit::PoolCount(_) => ("browser.pool", "browser.pool.pick"),
            BrowserHit::PoolMenu => ("browser.pool_menu", "browser.pool.menu"),
            BrowserHit::Audition => ("browser.audition", "browser.audition"),
            BrowserHit::Loc(_) => ("browser.loc", "browser.loc.pick"),
            BrowserHit::LocRemove(_) => ("browser.loc_remove", "browser.loc.remove"),
            BrowserHit::LocAdd => ("browser.loc_add", "browser.loc.add"),
            BrowserHit::Crumb(_) => ("browser.crumb", "browser.crumb"),
            BrowserHit::StopScan => ("browser.stop_scan", "plugin.scan.stop"),
            BrowserHit::Retry(_) => ("browser.retry", "plugin.scan.retry"),
        },
        Hit::Bar(b) => match b {
            BarButton::Logo => ("bar.logo", "about.open"),
            BarButton::SongName => ("bar.song", "song.menu"),
            BarButton::Saved => ("bar.saved", "version.list"),
            BarButton::Grid => ("bar.grid", "grid.menu"),
            BarButton::Snap => ("bar.snap", "view.snap"),
            BarButton::History => ("bar.history", "history.menu"),
            BarButton::Palette => ("bar.palette", "palette.open"),
            BarButton::Ai => ("bar.ai", "ai.menu"),
            BarButton::Export => ("bar.export", "export.open"),
            BarButton::Tool(_) => ("bar.tool", "tool.pick"),
            BarButton::AltTool => ("bar.alt_tool", "tool.alt.menu"),
            BarButton::Shape => ("bar.shape", "tool.draw.shape.menu"),
            BarButton::DrawLength => ("bar.draw_length", "prefs.set"),
            BarButton::ToStart => ("bar.to_start", "transport.to_start"),
            BarButton::Play => ("bar.play", "transport.play"),
            BarButton::Stop => ("bar.stop", "transport.stop"),
            BarButton::Record => ("bar.record", "transport.record"),
            BarButton::Loop => ("bar.loop", "transport.loop"),
            BarButton::PreRoll => ("bar.preroll", "record.preroll"),
            BarButton::RecPanel => ("bar.rec_panel", "view.record-panel"),
            BarButton::Cpu => ("bar.cpu", "cpu.list"),
            BarButton::Metronome => ("bar.metronome", "transport.metronome"),
            BarButton::Tempo => ("bar.tempo", "tempo.set"),
            BarButton::Meter => ("bar.meter", "meter.menu"),
            BarButton::Ripple => ("bar.ripple", "edit.ripple"),
        },
        Hit::Mixer(m) => match m {
            MixerHit::Strip(_) => ("mixer.strip", "track.select"),
            MixerHit::Fader(_) => ("mixer.fader", "track.volume"),
            MixerHit::Pan(_) => ("mixer.pan", "track.pan"),
            MixerHit::Mute(_) => ("mixer.mute", "track.mute"),
            MixerHit::Solo(_) => ("mixer.solo", "track.solo"),
            MixerHit::Arm(_) => ("mixer.arm", "track.arm"),
            MixerHit::Bypass(..) => ("mixer.bypass", "insert.bypass"),
            MixerHit::Open(..) => ("mixer.open", "insert.open"),
            MixerHit::AddInsert(_) => ("mixer.add_insert", "insert.add"),
            MixerHit::AddInstrument(_) => ("mixer.add_instrument", "instrument.add"),
            MixerHit::SendLevel(..) => ("mixer.send_level", "send.level"),
            MixerHit::SendPre(..) => ("mixer.send_pre", "send.prepost"),
            MixerHit::AddSend(_) => ("mixer.add_send", "send.add"),
            MixerHit::Output(_) => ("mixer.output", "track.output.menu"),
            MixerHit::Input(_) => ("mixer.input", "track.input.menu"),
        },
        Hit::Start(e) => match e {
            Entry::Audio => ("start.audio", "start.audio"),
            Entry::Midi => ("start.midi", "start.midi"),
            Entry::Drop => ("start.drop", "start.drop"),
        },
        Hit::Entrance(part) => match part {
            Part::Song(_) => ("start.recent", "song.open_recent"),
            Part::Sample => ("start.sample", "song.open_sample"),
            Part::Device => ("start.device", "audio.devices"),
            Part::Tone => ("start.tone", "audio.test-tone"),
        },
    };
    Some(m)
}

/// What a cell of a pop-over does. A press inside a panel with its own screen (export, plugin,
/// AI, quantize, about) is named by the panel: its parts are that panel's and appear nowhere else.
pub fn of_action(action: &Action) -> &'static str {
    match action {
        Action::OpenNoteEdit(_) => "noteedit.open",
        Action::EditValue(..) => "noteedit.value",
        Action::Link => "view.link",
        Action::Undo => "history.undo",
        Action::Redo => "history.redo",
        Action::Jump(_) => "history.jump",
        Action::StepInfo(_) => "history.detail",
        Action::Snap(_) => "view.snap",
        Action::GridValue(_) => "view.grid",
        Action::Triplet => "view.triplet",
        Action::Mode(_) => "view.snap_mode",
        Action::AutoQuantize => "view.auto_quantize",
        Action::AddTrack(_) => "track.add",
        Action::FromFile => "import.file",
        Action::OpenSong => "song.open",
        Action::OpenDawproject => "song.open_dawproject",
        Action::ExportDawproject => "export.dawproject",
        Action::NewSong => "song.new",
        Action::OpenRecent(_) => "song.open_recent",
        Action::Recovery(_) => "recovery.choose",
        Action::Save => "song.save",
        Action::SaveAs => "song.save_as",
        Action::ShowVersions => "version.list",
        Action::SongSettings => "project.settings",
        Action::Export => "export.open",
        Action::NameVersion => "version.name",
        Action::SelectVersion(_) => "version.select",
        Action::Restore => "version.restore",
        Action::OpenAsSong => "version.open_as_song",
        Action::Color(_) => "track.color.set",
        Action::Output(_) => "track.output.set",
        Action::Input(_) => "track.input.set",
        Action::View(_) => "browser.view.set",
        Action::Pick(_) => "pick.choose",
        Action::Xp(_) => "export.panel",
        Action::Plugin(_) => "plugin.panel",
        Action::Ai(_) => "ai.panel",
        Action::Quant(_) => "quantize.panel",
        Action::FolderBus(_) => "folder.bus.set",
        Action::RecQuantize(_) => "record.input-quantize.set",
        Action::Device(_) => "audio.set",
        Action::AutomationTarget(..) => "auto.target.set",
        Action::About(_) => "about.panel",
        Action::Ctx(_) => "context.row",
        Action::CtxSearch => "context.search",
        Action::Meter(_) => "timesig.set",
        Action::Report(_) => "preset.report",
        Action::Cleanup(crate::cleanup_dialog::Act::Trash) => "project.cleanup",
        Action::Cleanup(crate::cleanup_dialog::Act::Cancel) => "cleanup.cancel",
        Action::Cpu(_) => "cpu.row",
        Action::Field(..) => "context.field",
    }
}

/// What a cell does, in the pop-over of this kind: the pick lists of the insert, the send and the
/// instrument are three operations, not one.
pub fn of_cell(kind: &PopupKind, action: &Action) -> &'static str {
    match (kind, action) {
        (PopupKind::Insert(_), Action::Pick(_)) => "insert.pick",
        (PopupKind::Send(_), Action::Pick(_)) => "send.pick",
        (PopupKind::Instrument(_), Action::Pick(_)) => "instrument.pick",
        _ => of_action(action),
    }
}

/// An operation the approved mocks put in two places, with where and why.
pub struct Approved {
    pub op: &'static str,
    pub places: &'static [&'static str],
    pub ground: &'static str,
}

pub const APPROVED: &[Approved] = &[
    Approved {
        op: "export.open",
        places: &["bar.export", "popup.song"],
        ground: "上の帯の書き出しボタンと曲のメニューの「書き出し」: モックの daw-main.html が両方を描く（rev2-spec §13）",
    },
    Approved {
        op: "version.list",
        places: &["bar.saved", "popup.song"],
        ground: "保存の札と曲のメニューの「バージョン一覧…」: モックが両方を描く（rev4-spec §5）",
    },
    Approved {
        op: "record.preroll",
        places: &["bar.preroll", "rec.preroll"],
        ground: "下の帯のプリロールのボタンと録音パネルのスイッチ: モックの recording.html の 04 が両方を描く（ボタンは UI-300 で下の帯へ移した）",
    },
    Approved {
        op: "view.snap",
        places: &["bar.snap", "popup.grid"],
        ground: "上の帯のスナップのスイッチとグリッドのメニューのオン・オフ: モックの ui-feedback.html の 04（B）が、スイッチを分け、値のボタンは今のグリッドのメニューを開く形にした（UI-302）",
    },
    Approved {
        op: "track.mute",
        places: &["head.mute", "mixer.mute"],
        ground: "トラックの頭とミキサーの帯の M: モックの daw-main.html が両方を描く（同じトラックの同じ状態を指す）",
    },
    Approved {
        op: "track.solo",
        places: &["head.solo", "mixer.solo"],
        ground: "トラックの頭とミキサーの帯の S: 同上",
    },
    Approved {
        op: "track.arm",
        places: &["head.arm", "mixer.arm"],
        ground: "トラックの頭とミキサーの帯の R: 同上",
    },
    Approved {
        op: "track.output.menu",
        places: &["head.output", "mixer.output"],
        ground: "トラックの頭（広げたとき）とミキサーの帯の出力: 同上",
    },
];

/// One operation and the places it was found in, sorted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub op: &'static str,
    pub places: Vec<String>,
}

/// Operations reached from more than one place, that no [`APPROVED`] entry covers exactly.
pub fn unapproved(found: &[Found]) -> Vec<&Found> {
    found
        .iter()
        .filter(|f| f.places.len() > 1)
        .filter(|f| {
            !APPROVED
                .iter()
                .any(|a| a.op == f.op && a.places.iter().map(|p| p.to_string()).eq(sorted(f)))
        })
        .collect()
}

fn sorted(f: &Found) -> Vec<String> {
    let mut p = f.places.clone();
    p.sort();
    p
}

/// `docs/operations.md`, from what the scan found.
pub fn document(found: &[Found]) -> String {
    let mut out = String::new();
    out.push_str("# 画面の操作の一覧\n\n");
    out.push_str(
        "UI-004: 同じ操作を画面の 2 か所に置かない。この一覧は `crates/xirius-ui/src/operations.rs` の\n\
         試験が画面から読んだ結果で、手で直さない（`XIRIUS_WRITE_OPERATIONS=1 cargo test -p xirius-ui operations`\n\
         で書き直す）。2 か所以上にある操作は、承認したモックが両方を描くものだけで、下の「承認済みの重複」に根拠を書く。\n\n",
    );
    out.push_str("## 承認済みの重複\n\n| 操作 | 場所 | 根拠 |\n| --- | --- | --- |\n");
    for a in APPROVED {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            a.op,
            a.places
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", "),
            a.ground
        ));
    }
    out.push_str("\n## すべての操作\n\n| 操作 | 場所 |\n| --- | --- |\n");
    for f in found {
        out.push_str(&format!(
            "| `{}` | {} |\n",
            f.op,
            f.places
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push_str(&format!(
        "\n操作 {} 件、うち 2 か所にあるもの {} 件（すべて承認済み）。\n",
        found.len(),
        found.iter().filter(|f| f.places.len() > 1).count()
    ));
    out.push_str("\n保存（UI-288）: Ctrl+S と曲のメニューは同じ保存を開始する。場所が未定なら別名保存の窓を開く。書き込み中の印は少なくとも 450 ms 保ち、200 ms を超える待ちは幕に載せる。別名保存の幕だけに進捗の棒と「中止 Esc」を置く。保存の警告の札の「別名で保存…」は同じ窓を開き、「もう一度」は同じ保存を再試行する。札を閉じても、保存成功まで警告の印を保つ。\n");
    out.push_str("\nほかのアプリの変更（UI-289）: 札の「元に戻す」は Ctrl+Z（`history.undo`）と同じ操作で、「読み込む」は札だけにある。読み込みは履歴の 1 段「ほかのアプリの変更」として入り、元に戻す・やり直しで行き来する。\n");
    out
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::browser::RailTab;
    use crate::song::Song;
    use crate::studio::{Geometry, Studio};
    use crate::tree::RowKind;
    use xirius_design::Layout;

    type Seen = BTreeMap<&'static str, BTreeSet<String>>;

    fn note(seen: &mut Seen, op: &'static str, place: impl Into<String>) {
        seen.entry(op).or_default().insert(place.into());
    }

    /// Every press the pointer can make on the body and on the two bars, at a step of 4 px.
    fn scan(s: &Studio, g: &Geometry, seen: &mut Seen) {
        let mut y = -Layout::TOP_BAR_H + 1.0;
        while y < g.size.1 + Layout::BOTTOM_BAR_H {
            let mut x = 1.0;
            while x < g.size.0 {
                if let Some((place, op)) = of_hit(s.hit(g, x, y)) {
                    note(seen, op, place);
                }
                x += 4.0;
            }
            y += 4.0;
        }
    }

    fn popup_name(kind: &PopupKind) -> String {
        let d = format!("{kind:?}");
        let head = d.split(['(', ' ']).next().unwrap_or("").to_lowercase();
        format!("popup.{head}")
    }

    /// The cells of the pop-over that is open.
    fn cells(s: &Studio, g: &Geometry, seen: &mut Seen) {
        let Some(p) = s.popup.as_ref() else { return };
        let place = popup_name(&p.kind);
        for c in &p.layout(g.size).cells {
            if let Some(a) = &c.action {
                note(seen, of_cell(&p.kind, a), place.clone());
            }
        }
    }

    fn found() -> Vec<Found> {
        let mut seen = Seen::new();
        let (w, h) = (1600.0, 900.0);
        let mut states: Vec<Studio> = Vec::new();
        // The roll, the mixer, and each tab of the browser.
        let roll = Studio::sample();
        let mut mixer = Studio::sample();
        let g = mixer.geometry(w, h);
        mixer.press(&g, Hit::Tab(crate::studio::DrawerTab::Mixer));
        states.push(roll);
        states.push(mixer);
        for tab in RailTab::ALL {
            let mut s = Studio::sample();
            s.browser.open = true;
            s.browser.tab = tab;
            states.push(s);
        }
        // Every track widened: its output shows in its head too.
        let mut wide = Studio::sample();
        let g = wide.geometry(w, h);
        let tracks: Vec<_> = wide
            .tree()
            .rows
            .iter()
            .filter_map(|r| match r.kind {
                RowKind::Track(t) => Some(t),
                RowKind::Folder(_) => None,
            })
            .collect();
        for t in tracks {
            wide.press(&g, Hit::Widen(t));
        }
        states.push(wide);
        // The record panel, with auto-punch on so that the post-roll shows too.
        let mut rec = Studio::sample();
        rec.rec_panel.open = true;
        rec.rec_set("record.punch", serde_json::json!({ "on": true }));
        states.push(rec);
        // The entrance of the empty song.
        states.push(Studio::new(Song::empty()));
        // The arrangement as the editor: its bar has the ▾ of the alternative tool and the
        // ripple switch (EDT-135).
        let mut arrange = Studio::sample();
        arrange.arrange.focus = true;
        arrange.choose_editor();
        states.push(arrange);
        for s in &mut states {
            s.settle_pool();
            let g = s.geometry(w, h);
            scan(s, &g, &mut seen);
        }
        // Every pop-over a press opens.
        let sample = Studio::sample();
        let g = sample.geometry(w, h);
        let track = sample
            .tree()
            .rows
            .iter()
            .find_map(|r| match r.kind {
                RowKind::Track(t) => Some(t),
                RowKind::Folder(_) => None,
            })
            .expect("a track");
        let folder = sample
            .tree()
            .rows
            .iter()
            .find_map(|r| match r.kind {
                RowKind::Folder(f) => Some(f),
                RowKind::Track(_) => None,
            })
            .expect("a folder");
        let mut openers = vec![
            Hit::Bar(BarButton::Logo),
            Hit::Bar(BarButton::SongName),
            Hit::Bar(BarButton::Saved),
            Hit::Bar(BarButton::Grid),
            Hit::Bar(BarButton::Meter),
            Hit::Bar(BarButton::History),
            Hit::Bar(BarButton::Ai),
            Hit::Bar(BarButton::Export),
            Hit::AddTrack,
            Hit::TrackColor(track),
            Hit::Output(track),
            Hit::FolderBus(folder),
            Hit::Browser(BrowserHit::ViewButton),
            Hit::Mixer(MixerHit::Output(track)),
            Hit::Mixer(MixerHit::Input(track)),
            Hit::Mixer(MixerHit::AddInsert(track)),
            Hit::Mixer(MixerHit::AddInstrument(track)),
            Hit::Mixer(MixerHit::AddSend(track)),
        ];
        openers.dedup();
        for hit in openers {
            let mut s = Studio::sample();
            s.browser.open = true;
            let g = s.geometry(w, h);
            s.press(&g, hit);
            cells(&s, &g, &mut seen);
        }
        // The input quantize's values, from the record panel.
        let mut s = Studio::sample();
        s.rec_panel.open = true;
        let rg = s.geometry(w, h);
        s.press(&rg, Hit::Rec(crate::rec_panel::RecPart::QuantizeValue));
        cells(&s, &rg, &mut seen);
        // The versions list is opened from the song menu and from the saved chip.
        let mut s = Studio::sample();
        s.press(&g, Hit::Bar(BarButton::Saved));
        cells(&s, &g, &mut seen);
        seen.into_iter()
            .map(|(op, places)| Found {
                op,
                places: places.into_iter().collect(),
            })
            .collect()
    }

    fn doc_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/operations.md")
    }

    #[test]
    fn no_operation_is_on_the_screen_in_two_places_but_the_approved_ones() {
        let found = found();
        let bad = unapproved(&found);
        assert!(
            bad.is_empty(),
            "an operation in 2 places that no approved mock draws twice: {bad:#?}"
        );
    }

    #[test]
    fn the_scan_sees_the_whole_screen_and_the_approved_list_has_no_dead_entry() {
        let found = found();
        let ops: BTreeSet<&str> = found.iter().map(|f| f.op).collect();
        for want in [
            "transport.play",
            "track.mute",
            "track.add.menu",
            "track.add",
            "song.save",
            "history.jump",
            "browser.item.pick",
            "mixer",
        ] {
            if want == "mixer" {
                assert!(ops.contains("track.volume"), "the mixer was not scanned");
            } else {
                assert!(ops.contains(want), "{want} not found: {ops:?}");
            }
        }
        // Each approved overlap really is on the screen, in the places it says.
        for a in APPROVED {
            let f = found
                .iter()
                .find(|f| f.op == a.op)
                .unwrap_or_else(|| panic!("{} not found", a.op));
            assert!(
                a.places.iter().all(|p| f.places.iter().any(|q| q == p)),
                "{}: approved in {:?}, found in {:?}",
                a.op,
                a.places,
                f.places
            );
        }
    }

    #[test]
    fn a_new_second_place_is_caught() {
        let mut found = vec![
            Found {
                op: "track.mute",
                places: vec!["head.mute".into(), "mixer.mute".into()],
            },
            Found {
                op: "song.save",
                places: vec!["popup.song".into(), "bar.save".into()],
            },
        ];
        let bad = unapproved(&found);
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].op, "song.save");
        // An approved operation in a third place is caught too.
        found[0].places.push("popup.grid".into());
        assert_eq!(unapproved(&found).len(), 2);
    }

    #[test]
    fn the_list_in_docs_is_what_the_scan_found() {
        let text = document(&found());
        let path = doc_path();
        if std::env::var_os("XIRIUS_WRITE_OPERATIONS").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let on_disk = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert_eq!(
            on_disk, text,
            "docs/operations.md is out of date: XIRIUS_WRITE_OPERATIONS=1 cargo test -p xirius-ui operations"
        );
    }
}
