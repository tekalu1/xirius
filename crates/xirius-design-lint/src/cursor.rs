//! 押せる物はポインターの形を持つ（UI-385）。左の押下・クリック・ドラッグを受ける要素の連なり
//! （`div().on_mouse_down(MouseButton::Left, …)` など）には、同じ連なりに `.cursor(…)`
//! （`cursor_pointer` などを含む）を書く。親の要素が形を決めているなら、例外の印にそう書く。
use crate::{Rule, scan::Hit};
use syn::{
    Expr, ExprMethodCall,
    visit::{self, Visit},
};

/// 押せる物にする呼び出し。
const PRESS: [&str; 3] = ["on_mouse_down", "on_click", "on_drag"];

pub fn scan(file: &syn::File) -> Vec<Hit> {
    let mut scanner = Scanner { hits: Vec::new() };
    scanner.visit_file(file);
    scanner.hits
}

struct Scanner {
    hits: Vec<Hit>,
}

/// 左以外のボタン（右のメニュー・中のスクロール）は押せる物の印にしない。
fn other_button(call: &ExprMethodCall) -> bool {
    let Some(Expr::Path(p)) = call.args.first() else {
        return false;
    };
    p.path
        .segments
        .last()
        .is_some_and(|s| s.ident != "Left" && p.path.segments.len() > 1)
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
    fn visit_expr_method_call(&mut self, top: &'ast ExprMethodCall) {
        // 連なりの一番外から、根まで降りる。
        let mut calls = vec![top];
        let mut root = &*top.receiver;
        while let Expr::MethodCall(inner) = root {
            calls.push(inner);
            root = &*inner.receiver;
        }
        let has_cursor = calls
            .iter()
            .any(|c| c.method.to_string().starts_with("cursor"));
        // 根に近い物から見て、最初の押せる呼び出しの所に出す。
        let press = calls
            .iter()
            .rev()
            .find(|c| PRESS.contains(&c.method.to_string().as_str()) && !other_button(c));
        if let Some(press) = press
            && !has_cursor
        {
            let pos = press.method.span().start();
            self.hits.push(Hit {
                line: pos.line,
                col: pos.column + 1,
                anchor: pos.line,
                rule: Rule::Cursor,
                message: format!(
                    "`.{}` を受ける要素に `.cursor(…)` が無い: 押せる物は指の形（ドラッグは矢印）を持つ。親が形を決めているなら例外の印に理由を書く",
                    press.method
                ),
            });
        }
        // 引数（閉包の中の要素も）と根は、それぞれの連なりとして見る。
        for c in &calls {
            for arg in &c.args {
                self.visit_expr(arg);
            }
        }
        self.visit_expr(root);
    }
}
