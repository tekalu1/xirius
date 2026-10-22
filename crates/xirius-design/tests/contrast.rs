//! コントラストの検査。文字は 4.5:1 以上、図形は 3:1 以上（WCAG 2.x）。
//! ダーク・ライトの両方で、公開の API だけを使って確かめる。

use xirius_design::{
    ALLOWED, Accent, Category, Ink, LevelMeter, LineState, MIN_GRAPHIC_CONTRAST, MIN_TEXT_CONTRAST,
    PianoKey, StructureLine, Surface, Theme, Toggle, TrackColor, contrast_ratio, token_names,
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
