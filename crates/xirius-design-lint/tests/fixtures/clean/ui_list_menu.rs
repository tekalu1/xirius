// 一覧は PickList で作る。操作のメニュー（「その他」）は理由を付ければ Menu で作れる。何も出ない。
fn device_list(names: &[String]) -> PickList {
    PickList::new(vec![PickGroup::new("出力", names.iter().map(PickRow::new).collect())])
}

fn more_menu(items: Vec<Pick>) -> Menu {
    // design-lint: allow(list-menu) 操作のメニューで、選ぶ一覧ではない
    Menu::rich(items, 240.0)
}

#[cfg(test)]
mod tests {
    fn menu() -> Menu {
        Menu::rich(Vec::new(), 120.0)
    }
}
