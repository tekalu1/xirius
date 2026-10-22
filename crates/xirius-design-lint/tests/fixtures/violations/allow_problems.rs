// 例外の印そのものの誤り。山形つきの印は 1 つ前の行を指す。
fn allows() {
    // design-lint: allow(border)
    //~^ allow-without-reason
    let a = div().border_1(); //~ border

    // design-lint: allow(no-such-rule) 規則名が違う
    //~^ unknown-rule
    let b = 1;

    // design-lint: allow(border) 次の行に何も無いので、何も止めていない
    //~^ unused-allow
    let c = 2;

    // design-lint: allow(border) 次の行だけに効く
    let d = div().border_1();
    let e = div().border_1(); //~ border

    // design-lint: something-else
    //~^ unknown-rule
    let f = 3;
}
