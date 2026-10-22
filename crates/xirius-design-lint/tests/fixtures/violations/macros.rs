// マクロの中も見る。式として読めない物は、トークンのまま探す。
fn macros() {
    println!("{}", rgb(0x112233)); //~ raw-color
    let v = vec![div().border_1()]; //~ border
    unparseable!(=> "#112233"); //~ raw-color
    unparseable!(=> x.border_1()); //~ border
    unparseable!(=> x.rounded(4.0)); //~ raw-radius
}
