// 生の色。行末の印が、その行で出るはずの違反（書き方は tests/fixtures.rs）。
fn colors() {
    let a = rgb(0x336699); //~ raw-color
    let b = hsla(0.5, 0.5, 0.5, 1.0); //~ raw-color
    let c = Color::hex(0x336699); //~ raw-color
    let d = "#336699"; //~ raw-color
    let e = "rgba(0, 0, 0, 0.5)"; //~ raw-color
    let f = gpui::black(); //~ raw-color
    let g = Hsla { h: 0.0, s: 0.0, l: 0.0, a: 1.0 }; //~ raw-color
    let h = "#fff"; //~ raw-color
    let not_a_color = "#12";
    let also_not = "#ghijkl";
}
