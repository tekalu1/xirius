// 文字を薄くする道は、別名・ブロック・代入・関数を通しても見つかる（text-opacity）。
fn f(){ let mut c=ink; c.a=0.3; pen.text(s,size,weight,c,r,align); } //~ text-opacity
// ----
fn f(){ let c:Hsla={ let x=ink.opacity(0.3); x }; pen.text(s,size,weight,c,r,align); } //~ text-opacity
// ----
fn light(c:Hsla)->Hsla { c.opacity(0.3) } fn f(){ paint::text(w,cx,font,s,size,light(ink),at); } //~ text-opacity
// ----
fn f(){ let run=TextRun { color:ink.opacity(0.3), ..base }; } //~ text-opacity
// ----
fn f(){ div().text_color(ink).opacity(0.3); } //~ text-opacity
// ----
fn f(){ div().opacity(0.3).text_color(ink); } //~ text-opacity, translucent-text-surface
// ----
fn f(){let c=theme; pen.text(s,size,weight,c.ink.opacity(0.3),r,align);} //~ text-opacity
// ----
fn f(){let mut color=ink; color=ink.opacity(0.3); pen.text(s,size,weight,color,r,align);} //~ text-opacity
// ----
fn f(){let color=if flag {let x=ink.opacity(0.3); x} else {ink}; pen.text(s,size,weight,color,r,align);} //~ text-opacity
// ----
// 透ける面の上の文字（translucent-text-surface）: ブロックを抜けても、矩形の別名でも残る。
fn f(){{pen.round(r,fill.opacity(0.3),radius);} pen.text(s,size,weight,ink,r,align);} //~ translucent-text-surface
// ----
fn f(){div().bg(fill.opacity(0.3)).child("text");} //~ translucent-text-surface
// ----
fn f(){let label=r; pen.round(r,fill.opacity(0.3),radius); pen.text(s,size,weight,ink,label,align);} //~ translucent-text-surface
// ----
fn f(){pen.round(r,fill.opacity(0.3),radius); pen.text(s,size,weight,ink,r.inset(pad),align);} //~ translucent-text-surface
// ----
// 族の取り違え（color-family）: 文字の色の面、線の色の文字、色の付いた全面のパネル。
fn f(){pen.round(r,c.ink,radius);} //~ color-family
// ----
fn f(){pen.text(s,size,weight,c.ring,r,align);} //~ color-family
// ----
fn f(){div().w_full().h_full().bg(c.fill);} //~ color-family
// ----
fn f(){let c=theme; pen.round(r,c.ink,radius);} //~ color-family
// ----
fn f(){paint::text(w,cx,font,s,size,c.surface_1,at);} //~ color-family
// ----
fn f(){let run=TextRun {color:c.fill, ..base};} //~ color-family
// ----
// DAW の例外の色（exception-family）は、決まった描き場所の外では使えない。
fn f(){pen.round(r,c.meter_ok,radius);} //~ exception-family
// ----
fn f(){pen.text(s,size,weight,c.meter_ok,r,align);} //~ exception-family
// ----
fn f(){pen.round(r,c.tracks[0],radius);} //~ exception-family
// ----
fn f(){pen.text(s,size,weight,c.tracks[0],r,align);} //~ exception-family
// ----
fn f(){pen.round(r,c.wave_color,radius);} //~ exception-family
// ----
fn f(){pen.text(s,size,weight,c.wave_color,r,align);} //~ exception-family
// ----
fn f(){pen.round(r,c.alert_warning,radius);} //~ exception-family
// ----
fn f(){pen.text(s,size,weight,c.alert_warning,r,align);} //~ exception-family
// ---- crates/xirius-ui/src/arrange/paint.rs
// 描き場所のファイルでも、描く関数の外（ツールバー）では使えない。
fn toolbar(){let c=theme;pen.round(r,c.tracks[0],radius);} //~ exception-family
// ---- crates/xirius-ui/src/mixer/paint.rs
fn toolbar(){let c=theme;pen.round(r,c.meter_ok,radius);} //~ exception-family
// ---- crates/xirius-ui/src/draw.rs
fn toolbar(){let c=theme;pen.round(r,c.wave_color,radius);} //~ exception-family
// ---- crates/xirius-ui/src/topbar.rs
fn toolbar(){let c=theme;pen.round(r,c.alert_warning,radius);} //~ exception-family
