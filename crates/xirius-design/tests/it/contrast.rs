//! コントラストの検査。文字は 4.5:1 以上、図形は 3:1 以上（WCAG 2.x）。
//! ダーク・ライトの両方で、公開の API だけを使って確かめる。
//!
//! 3 つの表に分ける: 文字（4.5:1）・図形（3:1）・パレットの決まり（組・不透明・名前など）。
//! どの表も行ごとに名前を持ち、落ちた行をまとめて報告する。

use xirius_design::{
    ALLOWED, Accent, Category, Color, GridLine, GridTier, Ink, LevelMeter, LineState,
    MIN_GRAPHIC_CONTRAST, MIN_TEXT_CONTRAST, Overlay, PianoKey, StructureLine, Surface, SwitchFace,
    Theme, Toggle, TrackColor, WaveColor, contrast_ratio, token_names,
};

/// 中立の面（文字・図形が載る面）。塗りは除く。
const NEUTRAL: [Surface; 8] = [
    Surface::Paper,
    Surface::S0,
    Surface::S1,
    Surface::S2,
    Surface::S3,
    Surface::Hover,
    Surface::Sel,
    Surface::Clip,
];

/// 時間のグリッドの線が載る面: アレンジの行（地・hover・選択）、ロールの黒鍵の行、レーンの溝。
const GRID_SURFACES: [Surface; 6] = [
    Surface::Paper,
    Surface::Hover,
    Surface::Sel,
    Surface::S0,
    Surface::S1,
    Surface::S2,
];

/// 表の 1 行: 名前、前に描く色、その下の面。`opaque` なら両方が不透明であることも求める。
struct Row {
    name: String,
    fg: Color,
    face: Color,
    opaque: bool,
}

fn row(name: String, fg: Color, face: Color, opaque: bool) -> Row {
    Row {
        name,
        fg,
        face,
        opaque,
    }
}

/// 行を下限で確かめ、落ちた行の説明と最小の比を返す。
fn check_rows(rows: &[Row], min: f64) -> (Vec<String>, f64) {
    let mut failures = Vec::new();
    let mut worst = f64::MAX;
    for r in rows {
        let ratio = contrast_ratio(r.fg, r.face);
        worst = worst.min(ratio);
        if ratio < min {
            failures.push(format!("{}: {ratio:.2} < {min}", r.name));
        }
        if r.opaque && !(r.fg.is_opaque() && r.face.is_opaque()) {
            failures.push(format!("{}: 透過の色が混ざっている", r.name));
        }
    }
    (failures, worst)
}

/// CIE L*（0〜100）。面の段の検査（surface_steps.rs）と同じ物差し。
fn lightness(c: Color) -> f64 {
    let y = c.relative_luminance();
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        24389.0 / 27.0 * y
    }
}

/// 文字の表: 文字と、その下の面の組の全部。
fn text_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for theme in Theme::ALL {
        // 許された面と文字の組（Pair）。
        for &(surface, ink) in ALLOWED {
            let r = ink
                .on(surface)
                .expect("一覧にある組は作れる")
                .resolve(theme);
            rows.push(row(
                format!("{theme:?} {ink:?} on {surface:?}"),
                r.ink,
                r.surface,
                true,
            ));
        }
        // トラックの色で塗ったセクションの名前。
        for track in TrackColor::all() {
            let face = track.section(theme);
            rows.push(row(
                format!("{theme:?} section of track {}", track.index()),
                face.label,
                face.fill,
                true,
            ));
        }
        // 札（入）の文字と、切の札の文字。
        for t in Toggle::ALL {
            let face = t.on(theme);
            rows.push(row(
                format!("{theme:?} toggle {t:?} on"),
                face.label,
                face.fill,
                true,
            ));
        }
        let off = Toggle::off().resolve(theme);
        rows.push(row(
            format!("{theme:?} toggle off"),
            off.ink,
            off.surface,
            true,
        ));
        // ロールの音名（C4 など）: 白鍵の上と、光った鍵の上（UI-013）。
        rows.push(row(
            format!("{theme:?} note name on a white key"),
            PianoKey::label_on(theme, false),
            PianoKey::White.color(theme),
            false,
        ));
        rows.push(row(
            format!("{theme:?} note name on a lit key"),
            PianoKey::label_on(theme, true),
            Accent::FocusRing.color(theme),
            false,
        ));
    }
    rows
}

#[test]
fn every_text_reads_at_4_5_on_its_face_in_both_themes() {
    let rows = text_rows();
    let (failures, worst) = check_rows(&rows, MIN_TEXT_CONTRAST);
    println!("文字 × 面: {} 行、最小 {worst:.2}:1", rows.len());
    assert!(
        failures.is_empty(),
        "文字が 4.5:1 に届かない行: {failures:#?}"
    );
}

/// 図形の表: 中身・アクセント・線・メーター・鍵・スイッチと、その下の面の組の全部。
fn graphic_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    for theme in Theme::ALL {
        // 中身（ノート・波形・打点）・アクセント・分類の点・構造線は、中立の面の上で。
        for s in NEUTRAL {
            let face = s.color(theme);
            for track in TrackColor::all() {
                rows.push(row(
                    format!("{theme:?} track {} on {s:?}", track.index()),
                    track.content(theme),
                    face,
                    false,
                ));
            }
            for a in Accent::ALL {
                rows.push(row(
                    format!("{theme:?} {a:?} on {s:?}"),
                    a.color(theme),
                    face,
                    false,
                ));
            }
            for k in Category::ALL {
                rows.push(row(
                    format!("{theme:?} category {k:?} on {s:?}"),
                    k.color(theme),
                    face,
                    false,
                ));
            }
            for state in LineState::ALL {
                rows.push(row(
                    format!("{theme:?} structure line {state:?} on {s:?}"),
                    StructureLine::color(state, theme),
                    face,
                    false,
                ));
            }
        }
        // 選んだクリップの面には、クリップの中身と名前だけが載る。
        for track in TrackColor::all() {
            rows.push(row(
                format!("{theme:?} track {} on ClipSel", track.index()),
                track.content(theme),
                Surface::ClipSel.color(theme),
                false,
            ));
        }
        // 選んだトラックの頭とストリップ（UI-340）: トラックの色、フォーカスの輪と AI の線、
        // フォルダーの線、オートメーションの印（Blue の図形）が載る。古さの点（StaleDot）は
        // 書き出しの札にだけ出るので、ここには載らない。
        let track_sel = Surface::TrackSel.color(theme);
        let mut head: Vec<(String, Color)> = TrackColor::all()
            .map(|t| (format!("track {}", t.index()), t.content(theme)))
            .collect();
        head.extend(
            [Accent::FocusRing, Accent::AiLine].map(|a| (format!("{a:?}"), a.color(theme))),
        );
        head.extend(LineState::ALL.iter().map(|s| {
            (
                format!("structure line {s:?}"),
                StructureLine::color(*s, theme),
            )
        }));
        head.push((
            "automation mark (Blue)".to_string(),
            Ink::Blue.on(Surface::Sel).unwrap().ink_color(theme),
        ));
        for (name, g) in head {
            rows.push(row(
                format!("{theme:?} {name} on TrackSel"),
                g,
                track_sel,
                false,
            ));
        }
        // メーターの帯は、その溝の上で。
        let well = LevelMeter::well(theme);
        for stop in LevelMeter::default().gradient(theme).stops() {
            rows.push(row(
                format!("{theme:?} meter stop at {}", stop.at),
                stop.color,
                well,
                false,
            ));
        }
        // 白鍵と黒鍵。
        rows.push(row(
            format!("{theme:?} white key against black key"),
            PianoKey::White.color(theme),
            PianoKey::Black.color(theme),
            false,
        ));
        // 暗黙のミュートの文字（UI-376）は塗らないので、切の札の面との差を図形の 3:1 で見る。
        rows.push(row(
            format!("{theme:?} implied mute letter on S3"),
            Toggle::implied_mute(theme),
            Surface::S3.color(theme),
            true,
        ));
        // 切のスイッチの溝はどの面の上でも見え、つまみは溝の上で見える。
        for hovered in [false, true] {
            let face = SwitchFace::off(theme, hovered);
            for background in [
                Surface::Paper,
                Surface::S0,
                Surface::S1,
                Surface::S2,
                Surface::S3,
                Surface::Hover,
                Surface::Sel,
            ] {
                rows.push(row(
                    format!("{theme:?} off switch (hover {hovered}) track on {background:?}"),
                    face.track,
                    background.color(theme),
                    true,
                ));
            }
            rows.push(row(
                format!("{theme:?} off switch (hover {hovered}) knob on its track"),
                face.knob,
                face.track,
                true,
            ));
        }
        // ブラウザーのプレビューの波形と、脈打つ点（どの位相でも）。
        rows.push(row(
            format!("{theme:?} wave preview on S1"),
            WaveColor::preview(theme),
            Surface::S1.color(theme),
            false,
        ));
        let sel = Surface::Sel.color(theme);
        let phases = (0..=100).map(|i| {
            WaveColor::DOT_MIN + (WaveColor::DOT_MAX - WaveColor::DOT_MIN) * i as f32 / 100.0
        });
        for alpha in phases.chain([WaveColor::DOT_REST]) {
            let mut dot = WaveColor::rest(theme);
            dot.a = (alpha * 255.0).round() as u8;
            rows.push(row(
                format!("{theme:?} preview dot at alpha {alpha} on Sel"),
                dot.over(sel),
                sel,
                false,
            ));
        }
    }
    rows
}

#[test]
fn every_graphic_stands_out_at_3_on_its_face_in_both_themes() {
    let rows = graphic_rows();
    let (failures, worst) = check_rows(&rows, MIN_GRAPHIC_CONTRAST);
    println!("図形 × 面: {} 行、最小 {worst:.2}:1", rows.len());
    assert!(
        failures.is_empty(),
        "図形が 3:1 に届かない行: {failures:#?}"
    );
}

/// 表の写し（`tests/contrast.txt`）。色を変えると、どの行の比が動いたかが差分に出る。
const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/contrast.txt");
const WRITE: &str = "XIRIUS_WRITE_CONTRAST=1 cargo nextest run -p xirius-design contrast";

fn hex(c: Color) -> String {
    let rgb = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    if c.is_opaque() {
        rgb
    } else {
        format!("{rgb}{:02x}", c.a)
    }
}

fn snapshot() -> String {
    let mut out = format!(
        "# 試験が出す表で、手で直さない（{WRITE}）。\n# 表\t行\t前の色\t面の色\t比\t下限\n"
    );
    for (table, rows, min) in [
        ("text", text_rows(), MIN_TEXT_CONTRAST),
        ("graphic", graphic_rows(), MIN_GRAPHIC_CONTRAST),
    ] {
        for r in rows {
            out.push_str(&format!(
                "{table}\t{}\t{}\t{}\t{:.2}\t{min}\n",
                r.name,
                hex(r.fg),
                hex(r.face),
                contrast_ratio(r.fg, r.face),
            ));
        }
    }
    out
}

#[test]
fn the_contrast_table_matches_its_snapshot() {
    let want = snapshot();
    if std::env::var_os("XIRIUS_WRITE_CONTRAST").is_some() {
        std::fs::write(SNAPSHOT, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(SNAPSHOT)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if have == want {
        return;
    }
    let (mut a, mut b) = (have.lines(), want.lines());
    let mut n = 1;
    let (old, new) = loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) if x == y => n += 1,
            (x, y) => break (x.unwrap_or("（無い）"), y.unwrap_or("（無い）")),
        }
    };
    panic!(
        "tests/contrast.txt:{n} が表と違う（変えるつもりなら {WRITE}）\n  写し: {old}\n  表:   {new}"
    );
}

#[test]
fn the_palette_keeps_its_pairing_shape_and_naming_rules() {
    let mut failures: Vec<String> = Vec::new();
    let mut check = |ok: bool, rule: String| {
        if !ok {
            failures.push(rule);
        }
    };

    // 面と文字の組の決まり。
    for s in Surface::ALL {
        for t in [s.text(), s.strong_text()] {
            check(
                t.surface() == s && ALLOWED.contains(&(s, t.ink())),
                format!("{s:?} の既定の文字 {:?} が一覧に無い", t.ink()),
            );
        }
    }
    for s in NEUTRAL {
        for i in [Ink::Strong, Ink::Body, Ink::Blue, Ink::Alert, Ink::Ai] {
            check(
                i.on(s).is_some(),
                format!("中立の面 {s:?} に {i:?} が置けない"),
            );
        }
        check(
            Ink::OnFill.on(s).is_none(),
            format!("{s:?} の上に塗りの上の文字が置ける"),
        );
    }
    for s in [
        Surface::Fill,
        Surface::FillHover,
        Surface::Danger,
        Surface::DangerHover,
    ] {
        check(
            s.readable_inks() == vec![Ink::OnFill] && s.text().ink() == Ink::OnFill,
            format!("{s:?} には塗りの上の文字だけ"),
        );
    }
    for s in [Surface::ClipSel, Surface::TrackSel] {
        for i in [Ink::Strong, Ink::Body] {
            check(i.on(s).is_some(), format!("{s:?} に {i:?} が置けない"));
        }
    }
    let mut pairs: Vec<_> = ALLOWED.iter().map(|&(s, i)| (s as u8, i as u8)).collect();
    let n = pairs.len();
    pairs.sort_unstable();
    pairs.dedup();
    check(pairs.len() == n, "ALLOWED に同じ組が 2 度ある".to_string());

    for theme in Theme::ALL {
        // 面は不透明。
        for s in Surface::ALL {
            check(
                s.color(theme).is_opaque(),
                format!("{theme:?} {s:?} の面が透過"),
            );
        }
        // トラックの色は 16 の別の色で、分類の点はそのどれとも別。
        let tracks: Vec<Color> = TrackColor::all().map(|t| t.content(theme)).collect();
        let mut distinct: Vec<u32> = tracks.iter().map(|c| c.to_rgb_u32()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        check(
            tracks.len() == 16 && distinct.len() == 16,
            format!(
                "{theme:?}: トラックの色が 16 の別の色でない（{}）",
                distinct.len()
            ),
        );
        for k in Category::ALL {
            check(
                !tracks.contains(&k.color(theme)),
                format!("{theme:?}: 分類 {k:?} がトラックの色と同じ"),
            );
        }
        // グリッドの線は文字の色を薄く重ねたもの。小節 > 拍 > 分割 > 細かい線 の比が、
        // どの面の上でも保たれ、いちばん薄い段も面と見分けられる（UI-339）。
        for t in GridTier::ALL {
            check(
                !GridLine::color(t, theme).is_opaque(),
                format!("{theme:?} grid {t:?} が不透明"),
            );
        }
        for s in GRID_SURFACES {
            let face = s.color(theme);
            let ratios =
                GridTier::ALL.map(|t| contrast_ratio(GridLine::color(t, theme).over(face), face));
            check(
                ratios.windows(2).all(|p| p[0] > p[1] * 1.1)
                    && ratios[3] >= 1.1
                    && ratios[0] >= 2.2,
                format!("{theme:?} grid on {s:?}: {ratios:.2?}"),
            );
        }
        // 選んだトラックの面は、フェーダーのつまみ（ダークで surface_thumb、ライトで紙の面。
        // xirius-ui の paint.rs）と別の色で、明るさの段の差もある。
        let face = Surface::TrackSel.color(theme);
        let cap = match theme {
            Theme::Dark => Overlay::ScrollThumb.color(theme),
            Theme::Light => Surface::Paper.color(theme),
        };
        let gap = (lightness(face) - lightness(cap)).abs();
        check(
            face != cap && face != Overlay::ScrollThumb.color(theme) && gap >= 1.25,
            format!("{theme:?}: TrackSel とつまみの L* の差 {gap:.2}"),
        );
        // 暗黙のミュートの文字は、色だけでなく切の札の文字とも別の色。
        check(
            Toggle::implied_mute(theme) != Toggle::off().ink_color(theme),
            format!("{theme:?}: 暗黙のミュートの文字が切の札の文字と同じ"),
        );
    }
    check(
        TrackColor::new(17).index() == 1,
        "TrackColor は 16 で一巡しない".to_string(),
    );

    // 薄い文字・薄い線・薄い色の面のトークンは作らない（rev3・rev4）。トークンは決まった族に入る。
    for name in token_names() {
        for banned in ["weak", "faint", "tint", "code", "muted", "dim"] {
            check(!name.contains(banned), format!("禁止のトークン: {name}"));
        }
        check(
            [
                "surface_", "fill_", "ink_", "line_", "meter_", "track_", "wave_", "alert_",
            ]
            .iter()
            .any(|family| name.starts_with(family)),
            format!("族に入らないトークン: {name}"),
        );
    }
    for ink in Ink::ALL {
        let n = format!("{ink:?}").to_lowercase();
        check(
            !n.contains("weak") && !n.contains("faint") && !n.contains("muted"),
            format!("禁止の文字の名前: {n}"),
        );
    }

    assert!(
        failures.is_empty(),
        "パレットの決まりを破る行: {failures:#?}"
    );
}
