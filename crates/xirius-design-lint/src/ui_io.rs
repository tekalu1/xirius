//! Direct blocking boundaries in UI code. Synchronous adapters need a reason
//! at the individual call, never an exemption for an entire UI module.
use crate::{
    Rule,
    scan::{Hit, is_test_only},
};
use quote::ToTokens;
use syn::{
    spanned::Spanned,
    visit::{self, Visit},
};

pub(crate) fn scan(file: &syn::File, label: &str) -> Vec<Hit> {
    if label.ends_with("_shots.rs")
        || label.contains("_shots/")
        || label.ends_with("/shots.rs")
        || label.ends_with("/script.rs")
    {
        return Vec::new();
    }
    let mut scanner = Scanner {
        hits: Vec::new(),
        awaiting: false,
        modules: std::collections::HashSet::new(),
        functions: std::collections::HashSet::new(),
        types: std::collections::HashSet::new(),
        workers: if label.ends_with("/ancillary_io.rs") {
            ["submit".into(), "dispatch".into()].into_iter().collect()
        } else {
            std::collections::HashSet::new()
        },
    };
    struct Imports<'a>(&'a mut Scanner);
    impl<'ast> Visit<'ast> for Imports<'_> {
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.0.import(&item.tree, Vec::new());
        }
    }
    Imports(&mut scanner).visit_file(file);
    scanner.visit_file(file);
    scanner.hits
}
struct Scanner {
    hits: Vec<Hit>,
    awaiting: bool,
    modules: std::collections::HashSet<String>,
    functions: std::collections::HashSet<String>,
    types: std::collections::HashSet<String>,
    workers: std::collections::HashSet<String>,
}
impl Scanner {
    fn import(&mut self, tree: &syn::UseTree, mut prefix: Vec<String>) {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                self.import(&p.tree, prefix);
            }
            syn::UseTree::Group(g) => {
                for item in &g.items {
                    self.import(item, prefix.clone());
                }
            }
            syn::UseTree::Name(n) => self.alias(prefix, n.ident.to_string(), n.ident.to_string()),
            syn::UseTree::Rename(n) => {
                self.alias(prefix, n.ident.to_string(), n.rename.to_string())
            }
            syn::UseTree::Glob(_) => {
                if prefix == ["std", "fs"] {
                    self.modules.insert("fs".into());
                    for name in [
                        "read",
                        "read_to_string",
                        "read_dir",
                        "write",
                        "copy",
                        "rename",
                        "create_dir",
                        "create_dir_all",
                        "remove_dir",
                        "remove_dir_all",
                        "remove_file",
                        "metadata",
                        "symlink_metadata",
                        "canonicalize",
                        "hard_link",
                        "read_link",
                        "set_permissions",
                    ] {
                        self.functions.insert(name.into());
                    }
                    self.types.extend(["File".into(), "OpenOptions".into()]);
                }
            }
        }
    }
    fn alias(&mut self, mut prefix: Vec<String>, name: String, alias: String) {
        prefix.push(name.clone());
        if prefix.iter().any(|p| p == "ancillary_io")
            && matches!(name.as_str(), "submit" | "dispatch")
        {
            self.workers.insert(alias.clone());
        }
        if prefix == ["std", "fs"] {
            self.modules.insert(alias);
        } else if prefix.starts_with(&["std".into(), "fs".into()]) {
            if matches!(name.as_str(), "File" | "OpenOptions" | "DirEntry") {
                self.types.insert(alias);
            } else {
                self.functions.insert(alias);
            }
        } else if prefix == ["std", "process", "Command"] {
            self.types.insert(alias);
        }
    }
    fn hit(&mut self, span: proc_macro2::Span, name: &str) {
        let at = span.start();
        self.hits.push(Hit {
            line: at.line,
            col: at.column,
            anchor: at.line,
            rule: Rule::UiBlockingIo,
            message: format!("`{name}`: worker か同期専用の呼出しと根拠を明記する"),
        });
    }
}
impl<'ast> Visit<'ast> for Scanner {
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        if let Some(name) = item.path.segments.last()
            && matches!(
                name.ident.to_string().as_str(),
                "print" | "println" | "eprint" | "eprintln" | "dbg"
            )
        {
            self.hit(item.path.span(), &name.ident.to_string());
        }
        visit::visit_macro(self, item);
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !is_test_only(&item.attrs) {
            visit::visit_item_mod(self, item);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_test_only(&item.attrs) {
            visit::visit_item_fn(self, item);
        }
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !is_test_only(&item.attrs) {
            visit::visit_impl_item_fn(self, item);
        }
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if !is_test_only(&item.attrs) {
            visit::visit_item_impl(self, item);
        }
    }
    fn visit_expr_await(&mut self, item: &'ast syn::ExprAwait) {
        let old = self.awaiting;
        self.awaiting = true;
        visit::visit_expr_await(self, item);
        self.awaiting = old;
    }
    fn visit_expr_call(&mut self, item: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*item.func {
            let parts: Vec<_> = path
                .path
                .segments
                .iter()
                .map(|p| p.ident.to_string())
                .collect();
            let name = parts.last().map(String::as_str).unwrap_or("");
            let worker = ((parts.iter().any(|p| p == "ancillary_io")
                && matches!(name, "dispatch" | "submit"))
                || (parts.len() == 1 && self.workers.contains(name)))
                || (name == "spawn" && parts.iter().any(|p| p == "thread" || p == "Job"));
            if worker {
                for arg in &item.args {
                    if !matches!(arg, syn::Expr::Closure(_)) {
                        self.visit_expr(arg);
                    }
                }
                return;
            }
            if parts.starts_with(&["std".into(), "fs".into()])
                || parts.first().is_some_and(|p| self.modules.contains(p))
                || (parts.len() == 1 && self.functions.contains(name))
                || (parts.first().is_some_and(|p| self.types.contains(p))
                    && matches!(name, "new" | "open" | "create" | "create_new"))
                || (parts.starts_with(&["std".into(), "process".into()]) && name == "new")
                || (name == "sleep" && parts.iter().any(|p| p == "thread"))
            {
                self.hit(item.func.span(), &path.to_token_stream().to_string());
            }
        }
        visit::visit_expr_call(self, item);
    }
    fn visit_expr_method_call(&mut self, item: &'ast syn::ExprMethodCall) {
        let name = item.method.to_string();
        if name == "spawn" && !item.args.is_empty() {
            self.visit_expr(&item.receiver);
            for arg in &item.args {
                if !matches!(arg, syn::Expr::Closure(_)) {
                    self.visit_expr(arg);
                }
            }
            return;
        }
        if matches!(
            name.as_str(),
            "exists"
                | "is_file"
                | "is_dir"
                | "metadata"
                | "canonicalize"
                | "lock"
                | "recv_timeout"
                | "wait_timeout"
                | "flush_writes"
                | "disk_changed"
                | "reload_from_disk"
                | "read"
                | "read_exact"
                | "read_to_end"
                | "read_to_string"
                | "write"
                | "write_all"
                | "flush"
                | "sync_all"
                | "sync_data"
                | "seek"
                | "wait_while"
        ) || (name == "recv" && !self.awaiting)
            || name == "wait"
            || (matches!(name.as_str(), "join" | "spawn") && item.args.is_empty())
        {
            self.hit(item.method.span(), &name);
        }
        visit::visit_expr_method_call(self, item);
    }
}
