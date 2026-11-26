// 別の規則の印は止めない。
// design-lint: allow(raw-color) 色の例外
//~^ unused-allow
fn f() { div().border_1(); } //~ border
// ----
// 印の誤りを知らせる規則そのものは、止められない。
// design-lint: allow(unused-allow) 試し
//~^ unknown-rule
fn f() {}
// ----
// 区切りの線も、理由が無ければ止められない。
fn f() {
    // design-lint: allow(line)
    //~^ allow-without-reason
    let a = div().w(px(2.0)).h_full().bg(c); //~ line
}
