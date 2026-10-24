//! コントラストの検査。文字は 4.5:1 以上、図形は 3:1 以上（WCAG 2.x）。
//! ダーク・ライトの両方で、公開の API だけを使って確かめる。

use xirius_design::{
    ALLOWED, Accent, Category, GridLine, GridTier, Ink, LevelMeter, LineState,
    MIN_GRAPHIC_CONTRAST, MIN_TEXT_CONTRAST, PianoKey, StructureLine, Surface, Theme, Toggle,
    TrackColor, contrast_ratio, token_names,
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

#[test]
fn every_allowed_text_surface_pair_is_at_least_4_5() {
    let mut worst = f64::MAX;
    for theme in Theme::ALL {
        for &(surface, ink) in ALLOWED {
            let pair = ink.on(surface).expect("一覧にある組は作れる");
            let r = pair.resolve(theme);
            let ratio = contrast_ratio(r.ink, r.surface);
            worst = worst.min(ratio);
            assert!(
                ratio >= MIN_TEXT_CONTRAST,
                "{theme:?} {ink:?} on {surface:?}: {ratio:.2} < 4.5"
            );
        }
    }
    println!(
        "文字 × 面: {} 組 × 2 テーマ、最小 {worst:.2}:1",
        ALLOWED.len()
    );
}

#[test]
fn surfaces_and_inks_are_opaque() {
    for theme in Theme::ALL {
        for s in Surface::ALL {
            assert!(s.color(theme).is_opaque(), "{theme:?} {s:?}");
        }
        for &(s, i) in ALLOWED {
            assert!(
                i.on(s).unwrap().ink_color(theme).is_opaque(),
                "{theme:?} {i:?}"
            );
        }
    }
}

#[test]
fn every_surface_has_default_texts() {
    for s in Surface::ALL {
        for t in [s.text(), s.strong_text()] {
            assert_eq!(t.surface(), s);
            assert!(
                ALLOWED.contains(&(s, t.ink())),
                "{s:?} の既定の文字 {:?} が一覧に無い",
                t.ink()
            );
        }
    }
}

#[test]
fn every_neutral_surface_allows_the_core_inks() {
    for s in NEUTRAL {
        for i in [Ink::Strong, Ink::Body, Ink::Blue, Ink::Alert, Ink::Ai] {
            assert!(i.on(s).is_some(), "{i:?} on {s:?}");
        }
    }
}

#[test]
fn fills_take_only_the_fill_text() {
    for s in [Surface::Fill, Surface::FillHover] {
        assert_eq!(s.readable_inks(), vec![Ink::OnFill]);
    }
    for s in NEUTRAL {
        assert!(
            Ink::OnFill.on(s).is_none(),
            "{s:?} の上に塗りの上の文字は置けない"
        );
    }
}

#[test]
fn text_on_every_track_colored_section_is_at_least_4_5() {
    let mut worst = f64::MAX;
    for theme in Theme::ALL {
        for track in TrackColor::all() {
            let face = track.section(theme);
            assert!(face.fill.is_opaque() && face.label.is_opaque());
            let ratio = contrast_ratio(face.label, face.fill);
            worst = worst.min(ratio);
            assert!(
                ratio >= MIN_TEXT_CONTRAST,
                "{theme:?} track {}: {ratio:.2} < 4.5",
                track.index()
            );
        }
    }
    println!("色で塗ったセクションの文字: 16 色 × 2 テーマ、最小 {worst:.2}:1");
}

#[test]
fn there_are_sixteen_distinct_track_colors() {
    for theme in Theme::ALL {
        let mut seen: Vec<u32> = TrackColor::all()
            .map(|t| t.content(theme).to_rgb_u32())
            .collect();
        assert_eq!(seen.len(), 16);
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 16, "{theme:?}: 同じ色が混ざっている");
    }
    assert_eq!(TrackColor::new(17).index(), 1);
}

#[test]
fn track_content_is_visible_on_every_neutral_surface() {
    // 中身（ノート・波形・打点）は、中立のクリップの面とその背景の上で 3:1 以上。
    let mut worst = f64::MAX;
    for theme in Theme::ALL {
        for track in TrackColor::all() {
            for s in NEUTRAL {
                let ratio = contrast_ratio(track.content(theme), s.color(theme));
                worst = worst.min(ratio);
                assert!(
                    ratio >= MIN_GRAPHIC_CONTRAST,
                    "{theme:?} track {} on {s:?}: {ratio:.2} < 3",
                    track.index()
                );
            }
        }
    }
    println!("トラックの色 × 中立の面: 最小 {worst:.2}:1");
}

#[test]
fn the_selected_clip_face_keeps_its_content_and_name_visible() {
    // 選んだクリップの面は、クリップの中身（ノート・波形・打点）と名前だけが載る。
    let mut worst = f64::MAX;
    for theme in Theme::ALL {
        let face = Surface::ClipSel.color(theme);
        for track in TrackColor::all() {
            let ratio = contrast_ratio(track.content(theme), face);
            worst = worst.min(ratio);
            assert!(
                ratio >= MIN_GRAPHIC_CONTRAST,
                "{theme:?} track {} on ClipSel: {ratio:.2} < 3",
                track.index()
            );
        }
    }
    for i in [Ink::Strong, Ink::Body] {
        assert!(i.on(Surface::ClipSel).is_some(), "{i:?} on ClipSel");
    }
    println!("トラックの色 × 選んだクリップの面: 最小 {worst:.2}:1");
}

#[test]
fn the_selected_track_face_keeps_the_head_visible_and_differs_from_the_fader_cap() {
    // 選んだトラックの頭とストリップ（UI-340）: 名前（Strong）と本文（Body）、トラックの色、
    // フォーカスの輪と AI の線、フォルダーの線、オートメーションの印（Blue の図形）が載る。
    // 古さの点（StaleDot）は書き出しの札にだけ出るので、ここには載らない。
    let mut worst = f64::MAX;
    for theme in Theme::ALL {
        let face = Surface::TrackSel.color(theme);
        let mut graphics: Vec<_> = TrackColor::all().map(|t| t.content(theme)).collect();
        graphics.extend([Accent::FocusRing, Accent::AiLine].map(|a| a.color(theme)));
        graphics.extend(
            LineState::ALL
                .iter()
                .map(|s| StructureLine::color(*s, theme)),
        );
        graphics.push(Ink::Blue.on(Surface::Sel).unwrap().ink_color(theme));
        for g in graphics {
            let ratio = contrast_ratio(g, face);
            worst = worst.min(ratio);
            assert!(
                ratio >= MIN_GRAPHIC_CONTRAST,
                "{theme:?} {g:?} on TrackSel: {ratio:.2} < 3"
            );
        }
        // フェーダーのつまみはダークで surface_thumb、ライトで紙の面（xirius-ui の paint.rs）。
        let cap = match theme {
            Theme::Dark => xirius_design::Overlay::ScrollThumb.color(theme),
            Theme::Light => Surface::Paper.color(theme),
        };
        assert_ne!(face, cap, "{theme:?}");
        assert_ne!(
            face,
            xirius_design::Overlay::ScrollThumb.color(theme),
            "{theme:?}"
        );
        let gap = (lightness(face) - lightness(cap)).abs();
        assert!(gap >= 1.25, "{theme:?}: つまみとの L* の差 {gap:.2}");
    }
    for i in [Ink::Strong, Ink::Body] {
        assert!(i.on(Surface::TrackSel).is_some(), "{i:?} on TrackSel");
    }
    println!("頭に載る図形 × 選んだトラックの面: 最小 {worst:.2}:1");
}

/// CIE L*（0〜100）。面の段の検査（surface_steps.rs）と同じ物差し。
fn lightness(c: xirius_design::Color) -> f64 {
    let y = c.relative_luminance();
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        24389.0 / 27.0 * y
    }
}

#[test]
fn accents_are_visible_on_every_neutral_surface() {
    for theme in Theme::ALL {
        for a in Accent::ALL {
            for s in NEUTRAL {
                let ratio = contrast_ratio(a.color(theme), s.color(theme));
                assert!(
                    ratio >= MIN_GRAPHIC_CONTRAST,
                    "{theme:?} {a:?} on {s:?}: {ratio:.2} < 3"
                );
            }
        }
    }
}

#[test]
fn categories_are_visible_on_every_neutral_surface() {
    // 設定のキーボードの点（キーの面 surface-3 の上）と凡例の点。
    let mut worst = f64::MAX;
    for theme in Theme::ALL {
        for k in Category::ALL {
            for s in NEUTRAL {
                let ratio = contrast_ratio(k.color(theme), s.color(theme));
                worst = worst.min(ratio);
                assert!(
                    ratio >= MIN_GRAPHIC_CONTRAST,
                    "{theme:?} {k:?} on {s:?}: {ratio:.2} < 3"
                );
            }
        }
        let tracks: Vec<_> = TrackColor::all().map(|t| t.content(theme)).collect();
        for k in Category::ALL {
            assert!(
                !tracks.contains(&k.color(theme)),
                "{k:?} はトラックの色と別"
            );
        }
    }
    println!("分類 × 中立の面: 最小 {worst:.2}:1");
}

#[test]
fn meter_band_is_visible_on_its_well() {
    for theme in Theme::ALL {
        let well = LevelMeter::well(theme);
        let g = LevelMeter::default().gradient(theme);
        for stop in g.stops() {
            let ratio = contrast_ratio(stop.color, well);
            assert!(
                ratio >= MIN_GRAPHIC_CONTRAST,
                "{theme:?} at {}: {ratio:.2}",
                stop.at
            );
        }
    }
}

#[test]
fn no_weak_ink_token_exists() {
    // 薄い文字・薄い線・薄い色の面のトークンは、作らない（rev3・rev4）。
    for name in token_names() {
        for banned in ["weak", "faint", "tint", "code", "muted", "dim"] {
            assert!(!name.contains(banned), "禁止のトークン: {name}");
        }
    }
    for ink in Ink::ALL {
        let n = format!("{ink:?}").to_lowercase();
        assert!(
            !n.contains("weak") && !n.contains("faint") && !n.contains("muted"),
            "{n}"
        );
    }
}

#[test]
fn allowed_table_has_no_duplicates() {
    let mut v: Vec<_> = ALLOWED.iter().map(|&(s, i)| (s as u8, i as u8)).collect();
    let n = v.len();
    v.sort_unstable();
    v.dedup();
    assert_eq!(v.len(), n);
}

#[test]
fn structure_lines_are_visible_on_every_neutral_surface() {
    // 構造線（フォルダーの範囲の線）は、通常も hover も選択も、中立の面の上で 3:1 以上。
    let mut worst = f64::MAX;
    for theme in Theme::ALL {
        for state in LineState::ALL {
            for s in NEUTRAL {
                let ratio = contrast_ratio(StructureLine::color(state, theme), s.color(theme));
                worst = worst.min(ratio);
                assert!(
                    ratio >= MIN_GRAPHIC_CONTRAST,
                    "{theme:?} {state:?} on {s:?}: {ratio:.2} < 3"
                );
            }
        }
    }
    println!("構造線 × 中立の面: 最小 {worst:.2}:1");
}

/// 時間のグリッドの線が載る面: アレンジの行（地・hover・選択）、ロールの黒鍵の行、レーンの溝。
const GRID_SURFACES: [Surface; 6] = [
    Surface::Paper,
    Surface::Hover,
    Surface::Sel,
    Surface::S0,
    Surface::S1,
    Surface::S2,
];

#[test]
fn grid_tiers_keep_their_order_on_every_surface() {
    // 小節 > 拍 > 分割 > 細かい線 の比が、どの面の上でも両方のテーマで保たれる（UI-339）。
    // 線は透過の色なので、面に重ねてから比べる。いちばん薄い段も面と見分けられる。
    let mut rows = Vec::new();
    for theme in Theme::ALL {
        for s in GRID_SURFACES {
            let face = s.color(theme);
            let ratios =
                GridTier::ALL.map(|t| contrast_ratio(GridLine::color(t, theme).over(face), face));
            assert!(
                ratios.windows(2).all(|p| p[0] > p[1] * 1.1),
                "{theme:?} {s:?}: {ratios:.2?}"
            );
            assert!(ratios[3] >= 1.1, "{theme:?} {s:?}: {ratios:.2?}");
            assert!(ratios[0] >= 2.2, "{theme:?} {s:?}: {ratios:.2?}");
            rows.push(format!("{theme:?} {s:?} {ratios:.2?}"));
        }
    }
    println!(
        "グリッドの線 × 面:
{}",
        rows.join(
            "
"
        )
    );
}

#[test]
fn grid_lines_are_translucent_ink() {
    // 面の色ではなく、文字の色を薄く重ねる。面が変わっても段の差がほぼ同じになる。
    for theme in Theme::ALL {
        for t in GridTier::ALL {
            assert!(!GridLine::color(t, theme).is_opaque(), "{theme:?} {t:?}");
        }
    }
}

#[test]
fn toggle_faces_are_readable() {
    let mut worst = f64::MAX;
    for theme in Theme::ALL {
        for t in Toggle::ALL {
            let face = t.on(theme);
            assert!(face.fill.is_opaque() && face.label.is_opaque());
            let ratio = contrast_ratio(face.label, face.fill);
            worst = worst.min(ratio);
            assert!(
                ratio >= MIN_TEXT_CONTRAST,
                "{theme:?} {t:?}: {ratio:.2} < 4.5"
            );
        }
        let off = Toggle::off().resolve(theme);
        assert!(contrast_ratio(off.ink, off.surface) >= MIN_TEXT_CONTRAST);
    }
    println!("札（入）の文字: 最小 {worst:.2}:1");
}

#[test]
fn danger_surfaces_take_only_the_fill_text() {
    for s in [Surface::Danger, Surface::DangerHover] {
        assert_eq!(s.readable_inks(), vec![Ink::OnFill]);
        assert_eq!(s.text().ink(), Ink::OnFill);
    }
}

#[test]
fn piano_keys_are_told_apart() {
    for theme in Theme::ALL {
        let ratio = contrast_ratio(PianoKey::White.color(theme), PianoKey::Black.color(theme));
        assert!(ratio >= MIN_GRAPHIC_CONTRAST, "{theme:?}: {ratio:.2}");
    }
}
