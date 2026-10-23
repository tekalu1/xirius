//! 一覧のメニューは PickList で作る（UI-338）。ドロップダウンの `Menu` は「その他」の操作のメニューだけに使い、
//! 選ぶ一覧（設定の一覧も）を `Menu::rich` で作らない。
use crate::{Rule, scan::Hit};
use syn::{
    Expr, ExprCall,
    visit::{self, Visit},
};

/// `Menu` を定義する所と、PickList そのもの。
const HOMES: [&str; 2] = ["dropdown.rs", "pick_list.rs"];

pub fn scan(file: &syn::File, label: &str) -> Vec<Hit> {
    let label = label.replace('\\', "/");
    let name = label.rsplit('/').next().unwrap_or(&label);
    if HOMES.contains(&name) {
        return Vec::new();
    }
    let mut scanner = Scanner { hits: Vec::new() };
    scanner.visit_file(file);
    scanner.hits
}

struct Scanner {
    hits: Vec<Hit>,
}

impl<'ast> Visit<'ast> for Scanner {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !crate::scan::is_test_only(&node.attrs) {
            visit::visit_item_mod(self, node);
        }
    }
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !crate::scan::is_test_only(&node.attrs) {
            visit::visit_item_fn(self, node);
        }
    }
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if !crate::scan::is_test_only(&node.attrs) {
            visit::visit_item_impl(self, node);
        }
    }
    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if !crate::scan::is_test_only(&node.attrs) {
            visit::visit_impl_item_fn(self, node);
        }
    }
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(p) = &*call.func {
            let segments: Vec<String> = p
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if let [.., ty, f] = segments.as_slice()
                && ty == "Menu"
                && (f == "rich" || f == "new")
            {
                let pos = p.path.segments[segments.len() - 2].ident.span().start();
                self.hits.push(Hit {
                    line: pos.line,
                    col: pos.column + 1,
                    anchor: pos.line,
                    rule: Rule::ListMenu,
                    message: format!(
                        "`Menu::{f}`: 一覧は pick_list::PickList で作る（検索・↑↓・Enter・Esc・空の札がそろう）。操作のメニューなら例外の印に理由を書く"
                    ),
                });
            }
        }
        visit::visit_expr_call(self, call);
    }
}
