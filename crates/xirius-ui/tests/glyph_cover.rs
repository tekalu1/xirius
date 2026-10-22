//! Every character the screen writes is in one of the two bundled faces (UI-005), so none is left
//! to the OS fonts.
//!
//! The strings are the string and character literals of every crate's source outside its tests
//! (command names and their arguments, menu rows, the settings, the fixed text of the screen),
//! the command table's display names, and the keys of every preset as Windows and the Mac show
//! them. User text (track and file names) can hold anything and is not checked here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use syn::visit::{self, Visit};
use syn::{Attribute, ImplItemFn, Item, Lit, LitChar, LitStr, Macro};
use xirius_core::commands::labels::LABELS;
use xirius_prefs::catalog::PRESETS;
use xirius_prefs::{Keymap, Os};
use xirius_ui::fonts::EMBEDDED;
use xirius_ui::glyphs::missing;

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// A file that only holds tests (`#[cfg(test)] mod tests;` and the like).
fn test_file(path: &Path) -> bool {
    let stem = path.file_stem().unwrap().to_string_lossy();
    stem == "tests" || stem.ends_with("_tests")
}

fn has_ident(tokens: TokenStream, name: &str) -> bool {
    tokens.into_iter().any(|t| match t {
        TokenTree::Ident(i) => i == name,
        TokenTree::Group(g) => has_ident(g.stream(), name),
        _ => false,
    })
}

/// `#[test]`, or `#[cfg(test)]` and the like (but not `#[cfg(not(test))]`).
fn test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        a.path().is_ident("cfg")
            && a.meta.require_list().is_ok_and(|l| {
                has_ident(l.tokens.clone(), "test") && !has_ident(l.tokens.clone(), "not")
            })
    })
}

fn attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

/// The literals of a file, with their lines. Doc comments are attributes and are left out.
struct Literals {
    found: Vec<(usize, String)>,
}

impl Literals {
    fn tokens(&mut self, tokens: TokenStream) {
        let mut before: [Option<String>; 2] = [None, None];
        for t in tokens {
            match &t {
                TokenTree::Group(g) => self.tokens(g.stream()),
                // `#[doc = "…"]` inside a macro's body is a doc comment too.
                TokenTree::Literal(l) if before != [Some("doc".into()), Some("=".into())] => {
                    match Lit::new(l.clone()) {
                        Lit::Str(s) => self.visit_lit_str(&s),
                        Lit::Char(c) => self.visit_lit_char(&c),
                        _ => {}
                    }
                }
                _ => {}
            }
            before = [before[1].take(), Some(t.to_string())];
        }
    }
}

impl<'ast> Visit<'ast> for Literals {
    fn visit_attribute(&mut self, _: &'ast Attribute) {}

    fn visit_item(&mut self, item: &'ast Item) {
        if !test_only(attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
        if !test_only(&f.attrs) {
            visit::visit_impl_item_fn(self, f);
        }
    }

    fn visit_macro(&mut self, m: &'ast Macro) {
        visit::visit_macro(self, m);
        self.tokens(m.tokens.clone());
    }

    fn visit_lit_str(&mut self, s: &'ast LitStr) {
        self.found.push((s.span().start().line, s.value()));
    }

    fn visit_lit_char(&mut self, c: &'ast LitChar) {
        self.found
            .push((c.span().start().line, c.value().to_string()));
    }
}

fn report(problems: &mut Vec<String>, place: &str, text: &str) {
    let gone = missing(text);
    if !gone.is_empty() {
        problems.push(format!("{place}: {text:?} lacks {gone:?}"));
    }
}

#[test]
fn every_literal_of_the_source_is_in_a_bundled_face() {
    let root = crates_dir();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let src = entry.unwrap().path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    files.sort();
    assert!(
        files.len() > 100,
        "{} files under {}",
        files.len(),
        root.display()
    );

    let mut problems = Vec::new();
    let mut checked = 0;
    for path in files.iter().filter(|p| !test_file(p)) {
        let source = std::fs::read_to_string(path).unwrap();
        let file = syn::parse_file(&source)
            .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()));
        let mut lits = Literals { found: Vec::new() };
        lits.visit_file(&file);
        let name = path.strip_prefix(&root).unwrap().display().to_string();
        for (line, text) in lits.found.iter().filter(|(_, t)| !t.is_ascii()) {
            checked += 1;
            report(&mut problems, &format!("{name}:{line}"), text);
        }
    }
    assert!(
        checked > 1000,
        "only {checked} non-ASCII literals were found"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_command_label_is_in_a_bundled_face() {
    let mut problems = Vec::new();
    for (name, label) in LABELS {
        report(&mut problems, name, label);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_key_of_every_preset_is_in_a_bundled_face_on_both_systems() {
    let mut problems = Vec::new();
    let mut shown = BTreeSet::new();
    for preset in PRESETS {
        let keymap = Keymap {
            preset: preset.name.to_string(),
            ..Keymap::default()
        };
        for row in keymap.rows() {
            for key in row.keys.iter().chain(&row.preset_keys) {
                for os in [Os::Win, Os::Mac] {
                    let text = key.display(os);
                    report(
                        &mut problems,
                        &format!("{} {}", preset.name, row.command),
                        &text,
                    );
                    // The settings' search line puts the key in a Japanese sentence.
                    report(
                        &mut problems,
                        "keys",
                        &format!("{text} は割り当てられていません"),
                    );
                    shown.insert(text);
                }
            }
        }
    }
    assert!(
        shown.iter().any(|t| t.contains('⌘')),
        "no Mac key was shown"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn cmap(bytes: &[u8]) -> BTreeSet<u32> {
    FontRef::new(bytes)
        .unwrap()
        .charmap()
        .mappings()
        .filter(|(_, g)| *g != GlyphId::NOTDEF)
        .map(|(c, _)| c)
        .collect()
}

#[test]
fn the_weights_of_a_family_have_the_same_characters() {
    for family in ["Inter-", "NotoSansJP-"] {
        let maps: Vec<_> = EMBEDDED
            .iter()
            .filter(|(name, _)| name.starts_with(family))
            .map(|(name, bytes)| (name, cmap(bytes)))
            .collect();
        assert_eq!(maps.len(), 3, "{family}");
        for (name, map) in &maps[1..] {
            assert_eq!(map, &maps[0].1, "{name} differs from {}", maps[0].0);
        }
    }
}

/// Noto Sans JP is cut to JIS X 0208. Two of its characters, as Windows types them, are in
/// neither face: `≒` (row 2, U+2252) and `∥` (row 1, U+2225; the JIS table's `‖` U+2016 is in).
/// A string the user writes with them, or with a character outside JIS X 0208, still goes to the
/// OS fonts.
#[test]
fn the_two_faces_have_jis_x_0208_but_two() {
    let mut gone = Vec::new();
    let mut seen = 0;
    // Rows 1–8 (symbols, kana, Greek, Cyrillic, box drawing) and 16–84 (kanji) are JIS X 0208;
    // the rows between are the NEC and IBM additions the EUC-JP table also holds. The table maps
    // the cells the way Windows does.
    for row in (1..=8).chain(16..=84) {
        for cell in 1..=94u8 {
            let euc = [0xa0 + row, 0xa0 + cell];
            let (text, _, bad) = encoding_rs::EUC_JP.decode(&euc);
            let mut chars = text.chars();
            let (Some(ch), None, false) = (chars.next(), chars.next(), bad) else {
                continue;
            };
            seen += 1;
            gone.extend(missing(&ch.to_string()));
        }
    }
    assert!(seen > 6800, "{seen} characters");
    assert_eq!(gone, vec!['∥', '≒']);
}
