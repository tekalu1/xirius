// グラデーション。
fn fancy_faces() {
    let a = linear_gradient(0.0, from, to); //~ gradient
    let b = div().bg(gradient_fill()); //~ gradient
    let c = div().background_gradient(); //~ gradient
    let d = FancyGradient::new(); //~ gradient
}

struct HeaderGradient; //~ gradient
