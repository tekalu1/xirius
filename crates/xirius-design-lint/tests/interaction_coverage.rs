//! PRJ-031: an added text owner must be reviewed against the central interaction policy.
use std::{collections::BTreeSet, path::Path};
use syn::visit::{self, Visit};

struct TextTypes<'a>(bool, &'a BTreeSet<String>);
impl<'ast> Visit<'ast> for TextTypes<'_> {
    fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
        self.0 |= ty
            .path
            .segments
            .last()
            .is_some_and(|s| self.1.contains(&s.ident.to_string()));
        visit::visit_type_path(self, ty);
    }
}

fn owners(label: &str, source: &str) -> BTreeSet<String> {
    let file = syn::parse_file(source).unwrap();
    let mut known: BTreeSet<String> = [
        "Field", "Typing", "Form", "Ask", "Search", "Palette", "PickList",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    struct Aliases<'a>(&'a mut BTreeSet<String>);
    impl<'ast> Visit<'ast> for Aliases<'_> {
        fn visit_use_rename(&mut self, item: &'ast syn::UseRename) {
            if self.0.contains(&item.ident.to_string()) {
                self.0.insert(item.rename.to_string());
            }
        }
        fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
            let mut types = TextTypes(false, self.0);
            types.visit_type(&item.ty);
            if types.0 {
                self.0.insert(item.ident.to_string());
            }
        }
    }
    loop {
        let before = known.len();
        Aliases(&mut known).visit_file(&file);
        if known.len() == before {
            break;
        }
    }
    struct Scan<'a> {
        label: &'a str,
        known: &'a BTreeSet<String>,
        found: BTreeSet<String>,
    }
    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            if item.trait_.as_ref().is_some_and(|(_, path, _)| {
                path.segments
                    .last()
                    .is_some_and(|s| s.ident == "EntityInputHandler")
            }) {
                self.found
                    .insert(format!("{}:EntityInputHandler", self.label));
            }
            visit::visit_item_impl(self, item);
        }
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if !item
                .attrs
                .iter()
                .any(|a| a.path().is_ident("cfg") && quote::quote!(#a).to_string().contains("test"))
            {
                visit::visit_item_mod(self, item);
            }
        }
        fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
            for field in &item.fields {
                let Some(name) = &field.ident else { continue };
                let mut types = TextTypes(false, self.known);
                types.visit_type(&field.ty);
                if types.0
                    || matches!(
                        name.to_string().as_str(),
                        "query" | "marked" | "typing" | "naming"
                    )
                {
                    self.found
                        .insert(format!("{}:{}.{}", self.label, item.ident, name));
                }
            }
        }
    }
    let mut scan = Scan {
        label,
        known: &known,
        found: BTreeSet::new(),
    };
    scan.visit_file(&file);
    scan.found
}

fn scan_dir(root: &Path, dir: &Path, found: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan_dir(root, &path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_name().unwrap().to_string_lossy();
            if name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let label = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            found.extend(owners(&label, &std::fs::read_to_string(&path).unwrap()));
        }
    }
}

#[test]
fn a_new_text_owner_requires_an_interaction_review() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../xirius-ui/src");
    let mut found = BTreeSet::new();
    scan_dir(&root, &root, &mut found);
    // Storage, its owning interaction, and the central predicate protecting it.
    // This is deliberately an exact inventory: another field in an existing struct
    // also needs review, rather than silently inheriting the struct's old guard.
    let guards = [
        ("about.rs:View.query", "self.popup.is_some()"),
        ("bar_tempo.rs:BarEdit.typing", "self.bar_edit.active()"),
        ("browser.rs:Browser.typing", "self.browser.typing"),
        ("context/ask.rs:Ask.input", "self.popup.is_some()"),
        ("context/fields.rs:Typing.marked", "self.popup.is_some()"),
        ("context/mod.rs:State.ask", "self.popup.is_some()"),
        ("context/mod.rs:State.field", "self.popup.is_some()"),
        ("context/mod.rs:State.search", "self.popup.is_some()"),
        ("context/search.rs:Search.field", "self.popup.is_some()"),
        (
            "inspector.rs:Inspector.field",
            "self.inspector.field.is_some()",
        ),
        ("palette.rs:Palette.form", "self.palette.is_some()"),
        ("palette.rs:Palette.marked", "self.palette.is_some()"),
        ("palette.rs:Palette.query", "self.palette.is_some()"),
        ("palette_args.rs:Form.fields", "self.palette.is_some()"),
        (
            "pick_list.rs:PickList.marked",
            "self.ime_target().is_some()",
        ),
        ("pick_list.rs:PickList.query", "self.ime_target().is_some()"),
        ("popup.rs:Popup.naming", "self.popup.is_some()"),
        ("popup.rs:Popup.pick", "self.popup.is_some()"),
        // This is a selection snapshot, not text; the drag still owns it.
        ("roll/mod.rs:Drag.marked", "self.roll.drag.is_some()"),
        (
            "screen.rs:EntityInputHandler",
            "self.ime_target().is_some()",
        ),
        (
            "settings/field.rs:Field.marked",
            "self.ime_target().is_some()",
        ),
        ("settings/mod.rs:OpenMenu.pick", "self.settings.is_some()"),
        ("settings/mod.rs:SettingsUi.ask", "self.settings.is_some()"),
        (
            "settings/mod.rs:SettingsUi.field",
            "self.settings.is_some()",
        ),
        (
            "settings/mod.rs:SettingsUi.query",
            "self.settings.is_some()",
        ),
        ("studio.rs:Studio.palette", "self.palette.is_some()"),
        ("track_name.rs:Name.field", "self.input_map.name.is_some()"),
    ];
    let reviewed = guards.iter().map(|(owner, _)| owner.to_string()).collect();
    assert_eq!(
        found, reviewed,
        "Register the field's active owner in interaction.rs and add its MCP preservation test before updating this inventory"
    );
    let policy = std::fs::read_to_string(root.join("interaction.rs")).unwrap();
    let file = syn::parse_file(&policy).unwrap();
    let mut policy = String::new();
    for item in file.items {
        if let syn::Item::Impl(item) = item {
            for member in item.items {
                if let syn::ImplItem::Fn(f) = member
                    && matches!(
                        f.sig.ident.to_string().as_str(),
                        "human_interaction_active" | "ime_target"
                    )
                {
                    let body = f.block;
                    policy.push_str(&quote::quote!(#body).to_string());
                }
            }
        }
    }
    let policy: String = policy.chars().filter(|c| !c.is_whitespace()).collect();
    for (owner, guard) in guards {
        assert!(
            policy.contains(guard),
            "{owner} lost its central guard: {guard}"
        );
    }
}

#[test]
fn the_native_text_handler_cannot_add_a_separate_input_route() {
    struct Routes(Vec<String>);
    impl<'ast> Visit<'ast> for Routes {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if matches!(
                call.method.to_string().as_str(),
                "mark_text"
                    | "replace_text"
                    | "settings_text"
                    | "ask_text"
                    | "pick_text"
                    | "unmark"
                    | "settings_unmark"
                    | "ask_unmark"
                    | "pick_unmark"
            ) {
                self.0.push(call.method.to_string());
            }
            visit::visit_expr_method_call(self, call);
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../xirius-ui/src/screen.rs");
    let file = syn::parse_file(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut routes = Routes(Vec::new());
    for item in &file.items {
        if let syn::Item::Impl(item) = item
            && item
                .trait_
                .as_ref()
                .is_some_and(|(_, path, _)| path.is_ident("EntityInputHandler"))
        {
            routes.visit_item_impl(item);
        }
    }
    assert!(
        routes.0.is_empty(),
        "Use the exhaustive ImeTarget routing in interaction.rs: {:?}",
        routes.0
    );
}

#[test]
fn new_fields_and_custom_composition_or_search_owners_are_detected() {
    for source in [
        "struct NewEditor { field: Option<crate::settings::field::Field> }",
        "struct NewEditor { input: Field }",
        "struct NewEditor { typing: Option<CustomInput> }",
        "struct NewEditor { marked: String }",
        "struct NewEditor { query: String }",
        "use crate::settings::field::Field as Entry; struct NewEditor { input: Option<Entry> }",
        "type Entry = Field; type Next = Entry; struct NewEditor { input: Option<Next> }",
        "impl gpui::EntityInputHandler for NewEditor {}",
        "struct ExistingEditor { field: Option<Field>, second: Option<Field> }",
    ] {
        assert!(!owners("fixture.rs", source).is_empty(), "{source}");
    }
    assert_eq!(
        owners(
            "fixture.rs",
            "struct ExistingEditor { field: Option<Field>, second: Option<Field> }"
        )
        .len(),
        2
    );
}
