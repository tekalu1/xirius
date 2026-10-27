//! Direct blocking boundaries in UI code. Synchronous adapters need a reason
//! at the individual call, never an exemption for an entire UI module.
use crate::{
    Rule,
    scan::{Hit, is_test_only},
};
use quote::ToTokens;
use syn::parse::Parser;
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
        aliases: std::collections::HashMap::new(),
        workers: if label.ends_with("/ancillary_io.rs") {
            ["submit".into(), "dispatch".into()].into_iter().collect()
        } else {
            std::collections::HashSet::new()
        },
    };
    struct Imports<'a>(&'a mut Scanner);
    impl<'ast> Visit<'ast> for Imports<'_> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if !is_test_only(&item.attrs) {
                visit::visit_item_mod(self, item);
            }
        }
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
    aliases: std::collections::HashMap<String, Vec<String>>,
    workers: std::collections::HashSet<String>,
}
impl Scanner {
    fn macro_groups(&mut self, tokens: proc_macro2::TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        // Unknown macro grammars are not assumed safe. Inspect call-shaped
        // token slices as Rust expressions, as well as all nested groups.
        for start in 0..tokens.len() {
            if !matches!(&tokens[start], proc_macro2::TokenTree::Ident(_)) {
                continue;
            }
            for end in start + 1..tokens.len() {
                if let proc_macro2::TokenTree::Group(group) = &tokens[end] {
                    if group.delimiter() == proc_macro2::Delimiter::Parenthesis {
                        let candidate = tokens[start..=end].iter().cloned().collect();
                        if let Ok(expression) = syn::parse2::<syn::Expr>(candidate) {
                            self.visit_expr(&expression);
                        }
                    }
                    break;
                }
            }
        }
        for token in tokens {
            if let proc_macro2::TokenTree::Group(group) = token {
                if let Ok(expression) = syn::parse2::<syn::Expr>(group.stream()) {
                    self.visit_expr(&expression);
                } else {
                    self.macro_groups(group.stream());
                }
            }
        }
    }
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
    fn alias(&mut self, mut prefix: Vec<String>, name: String, mut alias: String) {
        if name == "self" && alias == "self" {
            alias = prefix.last().cloned().unwrap_or(alias);
        }
        let mut qualified = prefix.clone();
        if name != "self" {
            qualified.push(name.clone());
        }
        self.aliases.insert(alias.clone(), qualified);
        prefix.push(name.clone());
        if prefix == ["xirius_session", "background", "Job"] {
            self.workers.insert(alias.clone());
        }
        if (prefix.starts_with(&["crate".into(), "ancillary_io".into()])
            || prefix.first().is_some_and(|p| p == "ancillary_io"))
            && matches!(name.as_str(), "submit" | "dispatch")
        {
            self.workers.insert(alias.clone());
        }
        if prefix == ["std", "fs"]
            || prefix == ["std", "io"]
            || (matches!(prefix.as_slice(), [a, b, c] if a == "std" && (b == "fs" || b == "io") && c == "self"))
        {
            self.modules.insert(alias);
        } else if prefix.starts_with(&["std".into(), "fs".into()])
            || prefix.starts_with(&["std".into(), "io".into()])
        {
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
                "print" | "println" | "eprint" | "eprintln" | "dbg" | "write" | "writeln"
            )
        {
            self.hit(item.path.span(), &name.ident.to_string());
        }
        // syn's default visitor does not enter macro tokens. Standard expression
        // arguments (assert!, assert_eq!, format!, write!, etc.) retain spans.
        let arguments = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        if let Ok(expressions) = arguments.parse2(item.tokens.clone()) {
            for expression in &expressions {
                self.visit_expr(expression);
            }
        } else {
            self.macro_groups(item.tokens.clone());
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
            let mut parts: Vec<_> = path
                .path
                .segments
                .iter()
                .map(|p| p.ident.to_string())
                .collect();
            for _ in 0..8 {
                let Some(prefix) = parts.first().and_then(|p| self.aliases.get(p)) else {
                    break;
                };
                let qualified: Vec<_> =
                    prefix.iter().chain(parts.iter().skip(1)).cloned().collect();
                if qualified == parts {
                    break;
                }
                parts = qualified;
            }
            let name = parts.last().map(String::as_str).unwrap_or("");
            let worker = (((parts.first().is_some_and(|p| p == "ancillary_io")
                || parts.starts_with(&["crate".into(), "ancillary_io".into()]))
                && matches!(name, "dispatch" | "submit"))
                || (parts.len() == 1 && self.workers.contains(name)))
                || (name == "spawn"
                    && (parts == ["std", "thread", "spawn"]
                        || parts == ["thread", "spawn"]
                        || parts == ["xirius_session", "background", "Job", "spawn"]));
            if worker {
                for arg in &item.args {
                    if !matches!(arg, syn::Expr::Closure(_)) {
                        self.visit_expr(arg);
                    }
                }
                return;
            }
            if parts.starts_with(&["std".into(), "fs".into()])
                || (parts.iter().any(|p| p == "places")
                    && matches!(name, "load" | "save" | "run" | "first_run" | "default"))
                || (parts.iter().any(|p| p == "cleanup")
                    && matches!(name, "unused" | "run" | "tidy"))
                || matches!(
                    name,
                    "ready_installer" | "take_failure" | "processes_except_self"
                )
                || (parts.starts_with(&["std".into(), "io".into()])
                    && matches!(name, "stdout" | "stderr" | "stdin"))
                || matches!(
                    name,
                    "run_and_persist" | "store_selection" | "load_selection"
                )
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
        if name == "spawn"
            && !item.args.is_empty()
            && ["std :: thread :: Builder", "thread :: Builder"]
                .iter()
                .any(|prefix| {
                    item.receiver
                        .to_token_stream()
                        .to_string()
                        .starts_with(prefix)
                })
        {
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
                | "verify_with"
                | "at_launch"
                | "ask_ui"
                | "stored"
                | "run_value"
                | "ai_run"
                | "run_parsed"
                | "run_macro"
                | "arrange_run"
                | "run_command"
        ) || (name == "recv" && !self.awaiting)
            || (matches!(name.as_str(), "run" | "set" | "save" | "save_version") && {
                let receiver = item.receiver.to_token_stream().to_string();
                receiver.contains("song") || receiver.contains("session")
            })
            || name == "wait"
            || (matches!(name.as_str(), "join" | "spawn") && item.args.is_empty())
        {
            self.hit(item.method.span(), &name);
        }
        visit::visit_expr_method_call(self, item);
    }
}
