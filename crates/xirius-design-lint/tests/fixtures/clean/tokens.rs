// トークンだけで作った画面。何も出ない。
use xirius_design::{Font, Ink, LevelMeter, Radius, Surface, Theme, ValueGradient};

fn view(theme: Theme) {
    let panel = div()
        .bg(Surface::Panel.color(theme))
        .rounded(Radius::M.px())
        .text_color(Ink::Body.color(theme))
        .font_family(Font::Inter.family())
        .child(div().rounded_none());

    // メーターは、値を表す物なのでグラデーションを使える。
    let meter = LevelMeter::new(0.5);
    let gradient: ValueGradient = meter.gradient();

    // 浮く面の外側の影は、ぼかして全周に広げる。片側だけの影ではない。
    let float = BoxShadow {
        color: shadow_color,
        offset: point(px(0.0), px(12.0)),
        blur_radius: px(32.0),
        spread_radius: px(0.0),
    };
    // ずらしの無い（四辺に同じ）影も、片側だけの影ではない。
    let even = BoxShadow {
        color: shadow_color,
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: px(1.0),
    };

    // 太い面と、色の無い細い物は、線ではない。
    let swatch = div().w(px(24.0)).h(px(24.0)).bg(Surface::S1.color(theme));
    let spacer = div().w(px(8.0)).h(px(2.0));

    // 端に貼り付くが、色は付けない物・全面を覆う物は、1 辺の色ではない。
    let overlay = div().absolute().inset_0().bg(Surface::Veil.color(theme));
    let hit_area = div().absolute().left_0().w(px(3.0));
    let ok_string = "Interface is not a font name";
}

#[cfg(test)]
mod tests {
    // テストの中は検査しない。
    fn colors() {
        let a = rgb(0x336699);
        let b = div().border_1();
    }
}

#[test]
fn a_test() {
    let a = "#336699";
}
