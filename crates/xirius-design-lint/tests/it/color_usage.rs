use xirius_design_lint::{Rule, lint_source};

fn has(src: &str, rule: Rule) -> bool {
    lint_source("crates/xirius-ui/src/unexpected.rs", src)
        .iter()
        .any(|v| v.rule == rule)
}

#[test]
fn text_opacity_is_rejected_through_aliases_blocks_assignments_and_helpers() {
    for src in [
        "fn f(){ let mut c=ink; c.a=0.3; pen.text(s,size,weight,c,r,align); }",
        "fn f(){ let c:Hsla={ let x=ink.opacity(0.3); x }; pen.text(s,size,weight,c,r,align); }",
        "fn light(c:Hsla)->Hsla { c.opacity(0.3) } fn f(){ paint::text(w,cx,font,s,size,light(ink),at); }",
        "fn f(){ let run=TextRun { color:ink.opacity(0.3), ..base }; }",
        "fn f(){ div().text_color(ink).opacity(0.3); }",
        "fn f(){ div().opacity(0.3).text_color(ink); }",
        "fn f(){let c=theme; pen.text(s,size,weight,c.ink.opacity(0.3),r,align);}",
        "fn f(){let mut color=ink; color=ink.opacity(0.3); pen.text(s,size,weight,color,r,align);}",
        "fn f(){let color=if flag {let x=ink.opacity(0.3); x} else {ink}; pen.text(s,size,weight,color,r,align);}",
    ] {
        assert!(has(src, Rule::TextOpacity), "{src}");
    }
}

#[test]
fn opaque_cover_clears_the_tint_and_rect_aliases_keep_it() {
    assert!(!has(
        "fn f(){if flag {pen.round(r,fill.opacity(0.3),radius);} else {pen.text(s,size,weight,ink,r,align);}}",
        Rule::TranslucentTextSurface
    ));
    assert!(has(
        "fn f(){{pen.round(r,fill.opacity(0.3),radius);} pen.text(s,size,weight,ink,r,align);}",
        Rule::TranslucentTextSurface
    ));
    assert!(has(
        "fn f(){div().bg(fill.opacity(0.3)).child(\"text\");}",
        Rule::TranslucentTextSurface
    ));
    assert!(has(
        "fn f(){let label=r; pen.round(r,fill.opacity(0.3),radius); pen.text(s,size,weight,ink,label,align);}",
        Rule::TranslucentTextSurface
    ));
    assert!(has(
        "fn f(){pen.round(r,fill.opacity(0.3),radius); pen.text(s,size,weight,ink,r.inset(pad),align);}",
        Rule::TranslucentTextSurface
    ));
    assert!(!has(
        "fn f(){pen.round(r,fill.opacity(0.3),radius);pen.round(r,surface,radius);pen.text(s,size,weight,ink,r,align);}",
        Rule::TranslucentTextSurface
    ));
}

#[test]
fn families_reject_ink_planes_line_text_and_colored_panels() {
    for src in [
        "fn f(){pen.round(r,c.ink,radius);}",
        "fn f(){pen.text(s,size,weight,c.ring,r,align);}",
        "fn f(){div().w_full().h_full().bg(c.fill);}",
        "fn f(){let c=theme; pen.round(r,c.ink,radius);}",
        "fn f(){paint::text(w,cx,font,s,size,c.surface_1,at);}",
        "fn f(){let run=TextRun {color:c.fill, ..base};}",
    ] {
        assert!(has(src, Rule::ColorFamily), "{src}");
    }
}

#[test]
fn daw_exceptions_are_limited_to_their_painting_places() {
    for (path, color) in [
        ("arrange/paint.rs", "c.tracks[0]"),
        ("mixer/paint.rs", "c.meter_ok"),
        ("draw.rs", "c.wave_color"),
        ("topbar.rs", "c.alert_warning"),
    ] {
        let src = format!("fn toolbar(){{let c=theme;pen.round(r,{color},radius);}}");
        assert!(
            lint_source(&format!("crates/xirius-ui/src/{path}"), &src)
                .iter()
                .any(|v| v.rule == Rule::ExceptionFamily),
            "{path}: {color}"
        );
    }
    for color in [
        "c.meter_ok",
        "c.tracks[0]",
        "c.wave_color",
        "c.alert_warning",
    ] {
        let src = format!("fn f(){{pen.round(r,{color},radius);}}");
        assert!(has(&src, Rule::ExceptionFamily), "{src}");
        let src = format!("fn f(){{pen.text(s,size,weight,{color},r,align);}}");
        assert!(has(&src, Rule::ExceptionFamily), "{src}");
    }
}

/// UI-309: 有彩色の塗りの面の上は ink_on_fill。今の画面のコードは 0 件で、もとの
/// 不具合（入ったトグルの面に地の文字の色のアイコン）は落ちる。
#[test]
fn fill_ink_holds_on_the_ui_and_rejects_the_old_toggle() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../xirius-ui/src");
    let mut stack = vec![src.clone()];
    let mut seen = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                let rel = path.strip_prefix(&src).unwrap().to_string_lossy();
                let label = format!("crates/xirius-ui/src/{}", rel.replace('\\', "/"));
                let source = std::fs::read_to_string(&path).unwrap();
                let hits: Vec<_> = lint_source(&label, &source)
                    .into_iter()
                    .filter(|v| v.rule == Rule::FillInk)
                    .collect();
                assert!(hits.is_empty(), "{hits:?}");
                seen += 1;
            }
        }
    }
    assert!(seen > 100, "{seen}");

    let old = "fn toggle_button(pen:&mut Pen,c:&Colors,r:Rect,on:bool){\
        let face=pen.transition(motion,setting,r,key,on,c.surface_1,c.fill);\
        pen.round(r,face,Radius::S); pen.icon(icon,r,c.ink);}";
    assert!(has(old, Rule::FillInk));
    let fixed = "fn toggle_button(pen:&mut Pen,c:&Colors,r:Rect,on:bool){\
        let (face,ink)=pen.transition_pair(motion,setting,r,key,on,(c.surface_1,c.ink),c.primary);\
        pen.round(r,face,Radius::S); pen.icon(icon,r,ink);}";
    assert!(!has(fixed, Rule::FillInk));
    for ink in ["c.ink", "c.ink_strong", "c.ink_blue", "c.ink_on_toggle"] {
        for face in ["c.fill", "c.fill_hover", "c.danger"] {
            let src = format!(
                "fn f(){{pen.round(r,{face},radius);pen.text(s,size,weight,{ink},r,align);}}"
            );
            assert!(has(&src, Rule::FillInk), "{src}");
        }
    }
    for src in [
        "fn f(){pen.round(r,c.fill,radius);pen.text(s,size,weight,c.on_fill,r,align);}",
        "fn f(){let (face,ink)=c.mute_on;pen.round(r,face,radius);pen.text(s,size,weight,ink,r,align);}",
        "fn f(){let (face,ink)=c.solo_on;pen.round(r,face,radius);pen.text(s,size,weight,ink,r,align);}",
        "fn f(){pen.round(r,c.fill.opacity(0.2),radius);pen.text(s,size,weight,c.ink,other,align);}",
    ] {
        assert!(!has(src, Rule::FillInk), "{src}");
    }
}
