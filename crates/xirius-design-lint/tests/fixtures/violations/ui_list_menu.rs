// 選ぶ一覧をドロップダウンの Menu で作ると落ちる（UI-338）。
fn device_menu(names: &[String]) -> Menu {
    let rows = names.iter().map(|n| Pick::row(n.clone(), 0)).collect();
    Menu::rich(rows, 240.0) //~ list-menu
}

fn quick_menu(names: &[&str]) -> crate::dropdown::Menu {
    crate::dropdown::Menu::new(names) //~ list-menu
}
