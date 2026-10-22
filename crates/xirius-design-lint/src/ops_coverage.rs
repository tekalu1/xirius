//! Validate command invocations and dispatch tables against the application registry.
use crate::{Rule, scan::Hit};
use std::collections::HashSet;
use syn::{
    Expr, Lit,
    visit::{self, Visit},
};

pub fn scan(file: &syn::File) -> Vec<Hit> {
    let registry = xirius_session::commands::registry();
    let names: HashSet<_> = registry.all().iter().map(|def| def.name).collect();
    let mut scanner = Scanner {
        names,
        hits: Vec::new(),
        seen: HashSet::new(),
    };
    scanner.visit_file(file);
    scanner.hits
}

struct Scanner<'a> {
    names: HashSet<&'a str>,
    hits: Vec<Hit>,
    seen: HashSet<(usize, usize)>,
}
impl Scanner<'_> {
    fn command(&mut self, literal: &syn::LitStr) {
        let text = literal.value();
        let name = text.split_whitespace().next().unwrap_or("");
        if self.names.contains(name) {
            return;
        }
        let pos = literal.span().start();
        if self.seen.insert((pos.line, pos.column)) {
            self.hits.push(Hit {
                line: pos.line,
                col: pos.column,
                anchor: pos.line,
                rule: Rule::OpsCoverage,
                message: format!("`{name}` がコマンド表に無い"),
            });
        }
    }

    fn table(&mut self, value: &Expr) {
        match value {
            Expr::Reference(reference) => self.table(&reference.expr),
            Expr::Array(array) => {
                for item in &array.elems {
                    self.table(item);
                }
            }
            Expr::Tuple(tuple) => {
                if let Some(first) = tuple.elems.first() {
                    self.table(first);
                }
            }
            Expr::Lit(literal) => {
                if let Lit::Str(value) = &literal.lit {
                    self.command(value);
                }
            }
            Expr::Struct(value) => {
                for field in &value.fields {
                    if matches!(&field.member, syn::Member::Named(name) if name == "command" || name == "name")
                    {
                        self.table(&field.expr);
                    }
                }
            }
            _ => {}
        }
    }
}
impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        if crate::scan::is_test_only(&node.attrs) {
            return;
        }
        let name = node.ident.to_string();
        if matches!(
            name.as_str(),
            "COMMANDS" | "OPERATIONS" | "OPS" | "SCOPES" | "FIRST"
        ) || name.ends_with("_COMMANDS")
        {
            self.table(&node.expr);
        }
        visit::visit_item_const(self, node);
    }
    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        for field in &node.fields {
            if matches!(&field.member, syn::Member::Named(name) if name == "command") {
                self.table(&field.expr);
            }
        }
        visit::visit_expr_struct(self, node);
    }
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
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if matches!(
            node.method.to_string().as_str(),
            "run"
                | "run_command"
                | "run_binding"
                | "run_value"
                | "check"
                | "palette_query"
                | "open_palette_form"
        ) && let Some(Expr::Lit(lit)) = node.args.first()
            && let Lit::Str(literal) = &lit.lit
        {
            self.command(literal);
        }
        visit::visit_expr_method_call(self, node);
    }
}
