//! 一般, オーディオ, MIDI, プラグイン and 編集.
//!
//! The plugin page shows what the scan found (the browser's list, PLG-021) and drives the
//! same scan as the insert list does.

use std::path::PathBuf;

use xirius_design::{FontSize, Radius, Weight};
use xirius_plugins::Catalog;
use xirius_prefs::Settings;

use super::parts::{self, ROW_H};
use super::scene::{self, B};
use super::{Do, MenuKind, SettingsUi};
use crate::draw::Align;
use crate::studio::Rect;

/// Gives a row of `w` at `y`.
pub(super) fn row(x: f32, y: f32, w: f32) -> Rect {
    Rect::new(x, y, w, ROW_H)
}

/// "true" or "false", the way `prefs.set` takes a switch.
pub(super) fn flip(on: bool) -> String {
    (!on).to_string()
}

/// A row with a switch at its end.
#[allow(clippy::too_many_arguments)]
pub(super) fn switch_row(
    b: &mut B,
    s: &Settings,
    x: f32,
    y: f32,
    w: f32,
    icon: &'static str,
    label: &str,
    key: &str,
    tip: Option<&str>,
) -> f32 {
    let r = row(x, y, w);
    parts::label(b, r, Some(icon), label, None);
    let on = s.prefs.bool(key);
    parts::toggle(b, r.right(), r, on, Do::Pref(key.into(), flip(on)), tip);
    r.bottom()
}

/// A row with a segmented control at its end.
#[allow(clippy::too_many_arguments)]
fn seg_row(
    b: &mut B,
    s: &Settings,
    x: f32,
    y: f32,
    w: f32,
    icon: &'static str,
    label: &str,
    key: &str,
    choices: &[(&str, &str, &str)],
) -> f32 {
    let r = row(x, y, w);
    parts::label(b, r, Some(icon), label, None);
    let now = s.prefs.text(key);
    let segs: Vec<parts::Seg> = choices
        .iter()
        .map(|(v, l, tip)| parts::Seg {
            label: l,
            icon: None,
            act: Do::Pref(key.into(), v.to_string()),
            on: now == *v,
            tip: (!tip.is_empty()).then_some(*tip),
        })
        .collect();
    let sw = seg_width(&segs);
    parts::seg_with(b, r.right() - sw, r.y + 6.0, &segs, 24.0, 2.0, 30.0, 8.0);
    r.bottom()
}

/// How wide [`parts::seg_with`] makes these (with its usual sizes).
pub fn seg_width(segs: &[parts::Seg]) -> f32 {
    segs.iter()
        .map(|s| (scene::width(s.label, FontSize::Caption) + 16.0).max(30.0))
        .sum::<f32>()
        + 2.0 * segs.len().saturating_sub(1) as f32
        + 4.0
}

/// A row with a dropdown at its end.
#[allow(clippy::too_many_arguments)]
fn select_row(
    b: &mut B,
    s: &Settings,
    x: f32,
    y: f32,
    w: f32,
    icon: &'static str,
    label: &str,
    key: &str,
    field_w: f32,
) -> f32 {
    let r = row(x, y, w);
    parts::label(b, r, Some(icon), label, None);
    let now = s.prefs.text(key);
    let shown = choices(key)
        .into_iter()
        .find(|(v, _)| *v == now)
        .map(|(_, l)| l)
        .unwrap_or(now);
    let f = Rect::new(r.right() - field_w, r.y + 6.0, field_w, 28.0);
    b.field(f, &shown, true);
    b.item(f, Do::Menu(MenuKind::Pref(key.into())), None);
    r.bottom()
}

/// What a dropdown of a preference offers: (value, label).
pub fn choices(key: &str) -> Vec<(String, String)> {
    let pairs: &[(&str, &str)] = match key {
        "audio.driver" => &[
            ("wasapi-exclusive", "WASAPI（排他モード）"),
            ("wasapi-shared", "WASAPI（共有モード）"),
        ],
        "edit.draw.length" => &[("inspector", "クオンタイズの値"), ("last", "直前のノート")],
        k if k.starts_with("plugins.process.") => PROCESS,
        _ => &[],
    };
    pairs
        .iter()
        .map(|(v, l)| (v.to_string(), l.to_string()))
        .collect()
}

// ------------------------------------------------------------------ 一般

pub fn general(b: &mut B, s: &Settings, x: f32, top: f32, w: f32) -> f32 {
    let c = b.c;
    let mut y = parts::gh(b, "外観", x, top);
    y = seg_row(
        b,
        s,
        x,
        y,
        w,
        "sun",
        "テーマ",
        "ui.theme",
        &[
            ("light", "ライト", ""),
            ("dark", "ダーク", ""),
            ("auto", "自動", "OS の設定に合わせます"),
        ],
    );
    y = parts::gh(b, "AI", x, y);
    y = switch_row(
        b,
        s,
        x,
        y,
        w,
        "ai",
        "MCP サーバー",
        "ai.mcp.enabled",
        Some("AI のアプリからこのアプリを操作できるようにします"),
    );
    // The range it takes connections from (`ai.mcp.listen`) shows once the server listens on
    // the network too; it listens on this computer only (SET-008).
    // Whether a newer version is out, asked at launch (UI-262).
    y = parts::gh(b, "更新", x, y);
    let key = xirius_session::app::UPDATE_PREF;
    let r = row(x, y, w);
    parts::label(b, r, Some("sync"), "更新を確認", None);
    let on = s.prefs.bool(key);
    let switch = parts::toggle(
        b,
        r.right(),
        r,
        on,
        Do::Pref(key.into(), flip(on)),
        Some("1 日 1 回、GitHub のリリースを確認します"),
    );
    b.text(
        "起動時",
        FontSize::Caption,
        Weight::Regular,
        c.ink,
        Rect::new(r.x, r.y, switch.x - 8.0 - r.x, r.h),
        Align::Right,
    );
    let y = r.bottom();
    // UI-282: the beta channel, off unless the user wants the versions still being made.
    switch_row(
        b,
        s,
        x,
        y,
        w,
        "download",
        "ベータ版も受け取る",
        xirius_session::update::BETA_PREF,
        Some("開発中のバージョンを先に受け取ります"),
    )
}

// ---------------------------------------------------------------- プラグイン

/// A plugin of the page: one the scan found, or a bundle it could not use.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    pub name: String,
    pub vendor: String,
    pub format: &'static str,
    /// ARA plugins run in the host's process.
    pub ara: bool,
    pub path: PathBuf,
    /// Why it cannot be used; `None` for one the scan opened.
    pub bad: Option<&'static str>,
}

/// How far a running scan is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanView {
    pub done: u64,
    /// 0 until the scan knows how many bundles it has to open.
    pub total: u64,
    pub stopping: bool,
    /// The bundle it opens now; empty until it says.
    pub file: String,
}

/// What the plugin page shows: the list of the last scan and the scan that may be running.
#[derive(Clone, Debug, Default)]
pub struct PluginList {
    /// The plugins by name, then the bundles that could not be used.
    pub rows: Vec<Plugin>,
    pub scan: Option<ScanView>,
    /// The unusable bundle being tried again.
    pub retrying: Option<PathBuf>,
    /// Why the last scan failed.
    pub error: Option<String>,
    /// How far round (0..1) the spinners are.
    pub spin: f32,
}

impl PluginList {
    /// The list of `catalog`.
    pub fn of(catalog: &Catalog) -> PluginList {
        let format = |f: &str| {
            if f.eq_ignore_ascii_case("clap") {
                "CLAP"
            } else {
                "VST3"
            }
        };
        let mut rows: Vec<Plugin> = catalog
            .entries
            .iter()
            .map(|e| Plugin {
                name: e.name.clone(),
                vendor: e.vendor.clone(),
                format: format(&e.format),
                ara: e.ara,
                path: e.path.clone(),
                bad: None,
            })
            .collect();
        rows.sort_by_key(|p| p.name.to_lowercase());
        let mut bad: Vec<Plugin> = catalog
            .failures
            .iter()
            .map(|f| Plugin {
                name: f
                    .path
                    .file_stem()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                vendor: String::new(),
                format: format(
                    &f.path
                        .extension()
                        .map_or_else(String::new, |e| e.to_string_lossy().into_owned()),
                ),
                ara: false,
                path: f.path.clone(),
                bad: Some(crate::browser::failure_why(f.kind)),
            })
            .collect();
        bad.sort_by_key(|p| p.name.to_lowercase());
        rows.extend(bad);
        PluginList {
            rows,
            ..PluginList::default()
        }
    }

    /// The bundles that could not be used.
    pub fn bad_count(&self) -> usize {
        self.rows.iter().filter(|p| p.bad.is_some()).count()
    }
}

const PROCESS: &[(&str, &str)] = &[
    ("same", "同じプロセス"),
    ("shared", "共有プロセス"),
    ("dedicated", "専用プロセス"),
];

/// The folders the system keeps plugins in: always looked in, not removed.
pub const DEFAULT_FOLDERS: [(&str, &str); 2] = [
    ("plugins.vst3.paths", r"C:\Program Files\Common Files\VST3"),
    ("plugins.clap.paths", r"C:\Program Files\Common Files\CLAP"),
];

pub fn plugins(b: &mut B, ui: &SettingsUi, s: &Settings, x: f32, top: f32, w: f32) -> f32 {
    let c = b.c;
    let fc = super::faces(c);
    let mut y = parts::gh(b, "フォルダー", x, top) + 6.0;
    for (format, key) in [
        ("VST3", "plugins.vst3.paths"),
        ("CLAP", "plugins.clap.paths"),
    ] {
        b.text(
            format,
            FontSize::Caption,
            Weight::Semibold,
            c.ink,
            Rect::new(x, y, 52.0, 32.0),
            Align::Left,
        );
        let px = x + 52.0 + 12.0;
        let pw = w - 64.0;
        let fixed = DEFAULT_FOLDERS
            .iter()
            .filter(|(k, _)| *k == key)
            .map(|(_, p)| (p.to_string(), true));
        let user = s.prefs.list(key).into_iter().map(|p| (p, false));
        for (path, locked) in fixed.chain(user) {
            let r = Rect::new(px, y, pw, 32.0);
            b.round(r, fc.well, Radius::S);
            b.icon("folder", parts::small_icon_at(r.x + 10.0, r), c.ink);
            b.push_mask(Rect::new(r.x + 32.0, r.y, r.w - 32.0 - 30.0, r.h));
            b.text(
                &path,
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                Rect::new(r.x + 32.0, r.y, r.w - 62.0, r.h),
                Align::Left,
            );
            b.pop_mask();
            let end = Rect::new(r.right() - 4.0 - 22.0, r.y + 5.0, 22.0, 22.0);
            if locked {
                b.icon("lock", parts::small_icon_at(end.x + 4.0, end), c.ink);
                b.tip(end, "既定のフォルダー");
            } else {
                parts::icon_button(
                    b,
                    end,
                    "trash",
                    Do::ListRemove(key.into(), path.clone()),
                    Some("削除"),
                );
            }
            y = r.bottom() + 4.0;
        }
        let add = "フォルダーを追加";
        let r = Rect::new(px, y, parts::button_w(add), 28.0);
        parts::button(b, r, "plus", add, Do::AddFolder(key.into()));
        y = r.bottom() + 4.0;
    }
    y = parts::gh(b, "プラグイン", x, y);
    let bad = super::bad_count(ui);
    let bad_label = if bad > 0 {
        format!("読み込めない（{bad}）")
    } else {
        "読み込めない".to_string()
    };
    let r = row(x, y, w);
    let only = ui.bad_only && bad > 0;
    parts::seg(
        b,
        r.x,
        r.y + 6.0,
        &[
            ("すべて", Do::BadOnly(false), !only),
            (&bad_label, Do::BadOnly(true), only),
        ],
        None,
    );
    y = r.bottom();
    let list = &ui.plugins;
    if let Some(scan) = &list.scan {
        y = scanning_row(b, list.spin, scan, x, y, w);
    } else if let Some(e) = &list.error {
        let r = row(x, y, w);
        b.icon("alert", parts::small_icon_at(r.x + 10.0, r), c.alert);
        b.text(
            &format!("スキャンに失敗しました: {e}"),
            FontSize::Small,
            Weight::Regular,
            c.ink_alert,
            Rect::new(r.x + 34.0, r.y, r.w - 44.0, r.h),
            Align::Left,
        );
        y = r.bottom();
    }
    if list.rows.is_empty() && list.scan.is_none() {
        let r = row(x, y, w);
        b.text(
            "なし",
            FontSize::Small,
            Weight::Regular,
            c.ink,
            Rect::new(r.x + 10.0, r.y, r.w - 20.0, r.h),
            Align::Left,
        );
        return r.bottom();
    }
    for p in list.rows.iter() {
        if only && p.bad.is_none() {
            continue;
        }
        let r = Rect::new(x, y, w, if p.bad.is_some() { BAD_H } else { ROW_H });
        if b.hot(r) {
            let face = match c.theme {
                xirius_design::Theme::Dark => c.hover,
                xirius_design::Theme::Light => c.surface_1,
            };
            b.round(r, face, Radius::S);
        }
        let tail = Rect::new(r.right() - 4.0 - 28.0, r.y + (r.h - 28.0) / 2.0, 28.0, 28.0);
        let fmt = Rect::new(tail.x - 10.0 - 48.0, r.y, 48.0, r.h);
        let nx = r.x + 10.0;
        if let Some(why) = p.bad {
            // The name over the reason, as the browser's group of them reads.
            b.text(
                &p.name,
                FontSize::Small,
                Weight::Medium,
                c.ink_strong,
                Rect::new(nx, r.y + 4.0, fmt.x - nx - 10.0, 20.0),
                Align::Left,
            );
            b.icon(
                "alert",
                Rect::new(nx, r.y + 24.0 + 2.0, 12.0, 12.0),
                c.alert,
            );
            b.text(
                why,
                FontSize::Caption,
                Weight::Regular,
                c.ink_alert,
                Rect::new(nx + 18.0, r.y + 24.0, fmt.x - nx - 28.0, 16.0),
                Align::Left,
            );
            parts::chip(b, fmt.x, r, p.format);
            if list.retrying.as_ref() == Some(&p.path) {
                let s = 14.0;
                b.icon_turned(
                    "spinner",
                    Rect::new(
                        tail.x + (tail.w - s) / 2.0,
                        tail.y + (tail.h - s) / 2.0,
                        s,
                        s,
                    ),
                    c.ink_strong,
                    list.spin,
                );
            } else {
                parts::icon_button(b, tail, "sync", Do::Retry(p.path.clone()), Some("再試行"));
            }
            y = r.bottom();
            continue;
        }
        let nw = b.text(
            &p.name,
            FontSize::Small,
            Weight::Medium,
            c.ink_strong,
            Rect::new(nx, r.y, fmt.x - nx - 10.0, r.h),
            Align::Left,
        );
        b.text(
            &p.vendor,
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(
                nx + nw + 8.0,
                r.y + 1.0,
                (fmt.x - nx - nw - 18.0).max(0.0),
                r.h,
            ),
            Align::Left,
        );
        parts::chip(b, fmt.x, r, p.format);
        // The process a plugin runs in by default (`plugins.process.<name>`) shows once a new
        // insert takes it; an insert's own choice is in its menu (SET-008).
        y = r.bottom();
    }
    y
}

/// An unusable plugin's row: the name over the reason.
const BAD_H: f32 = 48.0;

/// The line on top of the list while a scan runs: that it runs, and the stop. How far it is
/// shows in the page's head (PLG-059).
fn scanning_row(b: &mut B, spin: f32, scan: &ScanView, x: f32, y: f32, w: f32) -> f32 {
    let c = b.c;
    let r = Rect::new(x, y, w, ROW_H + 8.0);
    let stop_w = scene::width("中止", FontSize::Caption) + 20.0;
    let stop = Rect::new(r.right() - 4.0 - stop_w, r.y + 6.0, stop_w, 28.0);
    b.icon_turned(
        "spinner",
        Rect::new(r.x + 10.0, r.y + 13.0, 14.0, 14.0),
        c.ink_strong,
        spin,
    );
    let text = if scan.stopping {
        "中止しています…"
    } else if scan.total == 0 {
        "検索中"
    } else {
        "スキャン中"
    };
    let tx = r.x + 10.0 + 14.0 + 8.0;
    b.text(
        text,
        FontSize::Small,
        Weight::Regular,
        c.ink_strong,
        Rect::new(tx, r.y, (stop.x - tx).max(0.0), 40.0),
        Align::Left,
    );
    if !scan.stopping {
        let hot = b.hot(stop);
        b.round(stop, if hot { c.hover } else { c.surface_3 }, Radius::S);
        b.text(
            "中止",
            FontSize::Caption,
            Weight::Medium,
            c.ink_strong,
            stop,
            Align::Center,
        );
        b.item(stop, Do::StopScan, None);
    }
    r.bottom()
}

// ------------------------------------------------------------------- 編集

enum Ctl {
    Switch(Option<&'static str>),
    /// A switch that keeps a choice: (on, off).
    Mapped(&'static str, &'static str),
    Seg(&'static [(&'static str, &'static str, &'static str)]),
    Select,
    Velocity,
}

/// The rows of 編集, by group: (key after `edit.`, icon, label, control).
#[allow(clippy::type_complexity)]
const EDIT: &[(&str, &[(&str, &str, &str, Ctl)])] = &[
    (
        "ツール",
        &[
            (
                "arrow.mode",
                "select",
                "選択ツールのノート操作",
                Ctl::Seg(&[
                    ("extended", "拡張", "ノートの上側でベロシティ、下側で分割"),
                    ("basic", "基本", "移動と長さの変更のみ"),
                ]),
            ),
            (
                "arrange.link",
                "range",
                "クリップの上半分で範囲を選択",
                Ctl::Switch(None),
            ),
            (
                "cursor.crosshair",
                "crosshair",
                "ツールのカーソルを十字にする",
                Ctl::Switch(None),
            ),
            (
                "drag.value-tip",
                "eye",
                "ドラッグ中に値を表示",
                Ctl::Switch(None),
            ),
            (
                "wheel.ruler",
                "zoom-in",
                "ルーラー上のホイール",
                Ctl::Seg(&[("zoom", "ズーム", ""), ("scroll", "スクロール", "")]),
            ),
        ],
    ),
    (
        "ノート",
        &[
            ("draw.length", "quant", "入力するノートの長さ", Ctl::Select),
            (
                "draw.velocity",
                "velocity",
                "入力するノートのベロシティ",
                Ctl::Velocity,
            ),
            (
                "audition",
                "listen",
                "ノートを試聴",
                Ctl::Switch(Some("選択・入力・移動したノートを鳴らします")),
            ),
            (
                "note.overlap",
                "layers",
                "ノートの重なりを防ぐ",
                Ctl::Mapped("trim", "allow"),
            ),
            (
                "record.input-quantize",
                "quant",
                "入力クオンタイズ",
                Ctl::Switch(Some("録音するノートをクオンタイズします")),
            ),
        ],
    ),
    (
        "クリップ",
        &[
            (
                "clip.no-overlap",
                "layers",
                "クリップの重なりを防ぐ",
                Ctl::Switch(None),
            ),
            (
                "ripple",
                "arrange",
                "リップル編集",
                Ctl::Switch(Some("削除や長さの変更に合わせて後ろのクリップを動かします")),
            ),
            (
                "automation.follows-clips",
                "line",
                "クリップと一緒にオートメーションを移動",
                Ctl::Switch(None),
            ),
            (
                "automation.locks-clips",
                "lock",
                "オートメーションの表示中はクリップを編集しない",
                Ctl::Switch(None),
            ),
        ],
    ),
    (
        "再生位置",
        &[
            (
                "click-empty.locate",
                "skip-back",
                // design-lint: allow(guidance) 設定の名前。何の操作の振る舞いを切り替えるかを言う
                "空白部分のクリックで再生位置を移動",
                Ctl::Switch(None),
            ),
            // 「再生位置を編集位置に合わせる」 (`cursor.follows-edit`) shows once it works (SET-008).
            (
                "autoscroll",
                "follow",
                "自動スクロール",
                Ctl::Seg(&[
                    ("turn-over", "ページ", ""),
                    ("center", "中央", ""),
                    ("left", "左端", ""),
                ]),
            ),
        ],
    ),
];

pub fn edit(b: &mut B, ui: &SettingsUi, s: &Settings, x: f32, top: f32, w: f32) -> f32 {
    let mut y = top;
    for (group, rows) in EDIT {
        y = parts::gh(b, group, x, y);
        for (k, icon, label, ctl) in rows.iter() {
            let key = format!("edit.{k}");
            y = match ctl {
                Ctl::Switch(tip) => {
                    let r = row(x, y, w);
                    parts::label(b, r, Some(icon), label, *tip);
                    let on = s.prefs.bool(&key);
                    parts::toggle(b, r.right(), r, on, Do::Pref(key, flip(on)), None);
                    r.bottom()
                }
                Ctl::Mapped(on_v, off_v) => {
                    let r = row(x, y, w);
                    parts::label(b, r, Some(icon), label, None);
                    let on = s.prefs.text(&key) == *on_v;
                    let to = if on { off_v } else { on_v };
                    parts::toggle(b, r.right(), r, on, Do::Pref(key, to.to_string()), None);
                    r.bottom()
                }
                Ctl::Seg(choices) => seg_row(b, s, x, y, w, icon, label, &key, choices),
                Ctl::Select => select_row(b, s, x, y, w, icon, label, &key, 200.0),
                Ctl::Velocity => {
                    let r = row(x, y, w);
                    parts::label(b, r, Some(icon), label, None);
                    let now = s.prefs.text(&key);
                    let segs = [
                        parts::Seg {
                            label: "固定",
                            icon: None,
                            act: Do::Pref(key.clone(), "inspector".into()),
                            on: now == "fixed" || now == "inspector",
                            tip: None,
                        },
                        parts::Seg {
                            label: "前回の値",
                            icon: None,
                            act: Do::Pref(key.clone(), "last".into()),
                            on: now == "last",
                            tip: None,
                        },
                    ];
                    let sw = seg_width(&segs);
                    let sx = r.right() - sw;
                    parts::seg_with(b, sx, r.y + 6.0, &segs, 24.0, 2.0, 30.0, 8.0);
                    if now == "fixed" || now == "inspector" {
                        let vk = "edit.draw.velocity-value";
                        parts::number(b, ui, sx - 8.0, r, vk, s.prefs.number(vk), None, None);
                    }
                    r.bottom()
                }
            };
        }
    }
    y
}
