// 書体名の文字列。
fn fonts() {
    let a = div().font_family("Inter"); //~ raw-font
    let b = "Noto Sans JP"; //~ raw-font
    let c = SharedString::from("Helvetica Neue"); //~ raw-font
    let d = ".SystemUIFont"; //~ raw-font
    let e = "sans-serif"; //~ raw-font
    let f = "Segoe Fluent Icons"; //~ raw-font
    let g = "Segoe UI Symbol"; //~ raw-font
    let h = "Apple Color Emoji"; //~ raw-font
    let not_a_font = "Interface";
    let also_not = "Inter is a font name inside a sentence";
}
