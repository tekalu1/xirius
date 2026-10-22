//! Colour usage at paint sinks, including local aliases of translucent colours.
use crate::{rule::Rule, scan::Hit};
use quote::ToTokens;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use syn::parse::Parser;
use syn::{
    Expr, ExprMethodCall, ImplItemFn, ItemFn, Local, Pat,
    visit::{self, Visit},
};

pub fn scan(file: &syn::File, label: &str) -> Vec<Hit> {
    let mut helpers = Helpers::default();
    helpers.visit_file(file);
    let mut v = Functions {
        label,
        hits: Vec::new(),
        helpers: helpers.0,
    };
    v.visit_file(file);
    v.hits
}

struct Functions<'a> {
    label: &'a str,
    hits: Vec<Hit>,
    helpers: HashSet<String>,
}
#[derive(Default)]
struct Helpers(HashSet<String>);
impl<'ast> Visit<'ast> for Helpers {
    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        if f.block.to_token_stream().to_string().contains("opacity") {
            self.0.insert(f.sig.ident.to_string());
        }
    }
}
impl<'ast> Visit<'ast> for Functions<'_> {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !crate::scan::is_test_only(&m.attrs) {
            visit::visit_item_mod(self, m);
        }
    }
    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        if !crate::scan::is_test_only(&f.attrs) {
            self.block(&f.block, &f.sig.ident.to_string());
        }
    }
    fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
        if !crate::scan::is_test_only(&f.attrs) {
            self.block(&f.block, &f.sig.ident.to_string());
        }
    }
}
impl Functions<'_> {
    fn block(&mut self, b: &syn::Block, function: &str) {
        let mut sinks = Sinks {
            label: self.label,
            function,
            aliases: HashMap::new(),
            hits: Vec::new(),
            faces: Vec::new(),
            layers: Vec::new(),
            pairs: HashMap::new(),
            read_pairs: Cell::new(false),
            helpers: &self.helpers,
        };
        sinks.visit_block(b);
        self.hits.extend(sinks.hits);
    }
}
struct Sinks<'a> {
    label: &'a str,
    function: &'a str,
    aliases: HashMap<String, String>,
    hits: Vec<Hit>,
    faces: Vec<String>,
    /// Opaque faces in the order they are painted: the rect, and whether it
    /// is a chromatic fill (UI-309).
    layers: Vec<Layer>,
    /// The names bound by `let (face, ink) = …`, each to its element.
    pairs: HashMap<String, String>,
    /// Whether names read through `pairs` too: only for the fill-ink rule, so
    /// the older rules see what they saw before it.
    read_pairs: Cell<bool>,
    helpers: &'a HashSet<String>,
}

fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}
fn contains(s: &str, names: &[&str]) -> bool {
    words(s).iter().any(|w| names.contains(w))
}
fn ink(s: &str) -> bool {
    contains(
        s,
        &[
            "Ink",
            "ink",
            "ink_strong",
            "ink_blue",
            "ink_alert",
            "ink_ai",
            "ink_on_toggle",
            "on_fill",
            "key_label",
        ],
    )
}
fn line(s: &str) -> bool {
    contains(
        s,
        &["StructureLine", "ring", "ai_line", "line", "line_focus"],
    )
}
fn meter(s: &str) -> bool {
    contains(s, &["LevelMeter"])
        || color_field(
            s,
            &[
                "meter_ok",
                "meter_hot",
                "meter_over",
                "ok",
                "hot",
                "over",
                "meter_ramp",
            ],
        )
}
fn color_field(s: &str, fields: &[&str]) -> bool {
    words(s)
        .windows(2)
        .any(|p| p[0] == "." && fields.contains(&p[1]))
}
fn track(s: &str) -> bool {
    contains(s, &["TrackColor", "tracks", "track_ink", "color_of"])
}
fn wave(s: &str) -> bool {
    contains(
        s,
        &[
            "WaveColor",
            "wave_ink",
            "waves",
            "wave_color",
            "wave_preview",
            "wave_rest",
        ],
    )
}
fn alert(s: &str) -> bool {
    contains(
        s,
        &[
            "Alert",
            "alert",
            "stale",
            "alert_error",
            "alert_warning",
            "alert_success",
        ],
    )
}
impl Sinks<'_> {
    fn block_result(&self, b: &syn::Block, depth: usize) -> String {
        let mut scope = Sinks {
            label: self.label,
            function: self.function,
            aliases: self.aliases.clone(),
            hits: Vec::new(),
            faces: Vec::new(),
            layers: Vec::new(),
            pairs: self.pairs.clone(),
            read_pairs: self.read_pairs.clone(),
            helpers: self.helpers,
        };
        for stmt in &b.stmts {
            scope.visit_stmt(stmt);
        }
        b.stmts
            .last()
            .and_then(|s| match s {
                syn::Stmt::Expr(e, None) => Some(scope.resolved(e, depth + 1)),
                _ => None,
            })
            .unwrap_or_default()
    }
    fn resolved(&self, e: &Expr, depth: usize) -> String {
        if depth > 8 {
            return String::new();
        }
        match e {
            Expr::Block(b) => {
                return self.block_result(&b.block, depth);
            }
            Expr::If(i) => {
                let then = self.block_result(&i.then_branch, depth);
                return then
                    + " "
                    + &i.else_branch
                        .as_ref()
                        .map_or(String::new(), |(_, e)| self.resolved(e, depth + 1));
            }
            Expr::Match(m) => {
                return m
                    .arms
                    .iter()
                    .map(|a| self.resolved(&a.body, depth + 1))
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            Expr::Closure(c) => return self.resolved(&c.body, depth + 1),
            _ => {}
        }
        if let Expr::Path(p) = e
            && let Some(id) = p.path.get_ident()
        {
            let id = id.to_string();
            if self.read_pairs.get()
                && let Some(value) = self.pairs.get(&id)
            {
                return value.clone();
            }
            if let Some(value) = self.aliases.get(&id) {
                return value.clone();
            }
        }
        struct Expand<'a, 'b> {
            sink: &'a Sinks<'b>,
            text: String,
        }
        impl<'ast> Visit<'ast> for Expand<'_, '_> {
            fn visit_expr_path(&mut self, e: &'ast syn::ExprPath) {
                if let Some(id) = e.path.get_ident()
                    && let Some(value) = self.sink.aliases.get(&id.to_string())
                {
                    self.text.push(' ');
                    self.text.push_str(value);
                }
                if e.path
                    .get_ident()
                    .is_some_and(|id| self.sink.helpers.contains(&id.to_string()))
                {
                    self.text.push_str(" opacity");
                }
                visit::visit_expr_path(self, e);
            }
        }
        let mut v = Expand {
            sink: self,
            text: e.to_token_stream().to_string(),
        };
        v.visit_expr(e);
        v.text
    }
    fn hit(&mut self, call: &ExprMethodCall, rule: Rule, message: &str) {
        let p = call.method.span().start();
        self.hits.push(Hit {
            line: p.line,
            col: p.column + 1,
            anchor: p.line,
            rule,
            message: message.into(),
        });
    }
}
fn translucent(s: &str) -> bool {
    s.contains("opacity") || s.contains("with_alpha") || s.contains(". a =")
}
impl<'ast> Visit<'ast> for Sinks<'_> {
    fn visit_expr_if(&mut self, e: &'ast syn::ExprIf) {
        self.visit_expr(&e.cond);
        let aliases = self.aliases.clone();
        let faces = self.faces.clone();
        let layers = self.layers.clone();
        self.visit_block(&e.then_branch);
        let then_faces = self.faces.clone();
        let then_layers = self.layers.clone();
        self.aliases = aliases.clone();
        self.faces = faces;
        self.layers = layers;
        if let Some((_, otherwise)) = &e.else_branch {
            self.visit_expr(otherwise);
        }
        for face in then_faces {
            if !self.faces.contains(&face) {
                self.faces.push(face);
            }
        }
        for layer in then_layers {
            if !self.layers.contains(&layer) {
                self.layers.push(layer);
            }
        }
        self.aliases = aliases;
    }
    fn visit_block(&mut self, b: &'ast syn::Block) {
        let aliases = self.aliases.clone();
        let pairs = self.pairs.clone();
        visit::visit_block(self, b);
        self.aliases = aliases;
        self.pairs = pairs;
    }
    fn visit_expr_match(&mut self, m: &'ast syn::ExprMatch) {
        self.visit_expr(&m.expr);
        for arm in &m.arms {
            let aliases = self.aliases.clone();
            let faces = self.faces.clone();
            let layers = self.layers.clone();
            let pairs = self.pairs.clone();
            visit::visit_arm(self, arm);
            self.aliases = aliases;
            self.faces = faces;
            self.layers = layers;
            self.pairs = pairs;
        }
    }
    fn visit_expr_for_loop(&mut self, e: &'ast syn::ExprForLoop) {
        // Each turn paints its own faces, at its own rects.
        let layers = self.layers.clone();
        visit::visit_expr_for_loop(self, e);
        self.layers = layers;
    }
    fn visit_local(&mut self, l: &'ast Local) {
        let pat = match &l.pat {
            Pat::Type(t) => &*t.pat,
            p => p,
        };
        if let Pat::Ident(p) = pat
            && let Some(i) = &l.init
        {
            let value = self.resolved(&i.expr, 0);
            let paired = self.paired(&i.expr);
            let name = p.ident.to_string();
            // A new `r` is not the face painted at the old one.
            self.layers.retain(|l| l.rect.written != name);
            // A value made from a pair (`let ink = if on { …; ink } else { … }`)
            // keeps what it reads through the pair, for the fill-ink rule.
            if paired == value {
                self.pairs.remove(&name);
            } else {
                self.pairs.insert(name.clone(), paired);
            }
            self.aliases.insert(name, value);
        }
        if let Pat::Tuple(t) = pat
            && let Some(i) = &l.init
        {
            let read = self.read_pairs.replace(true);
            for (n, elem) in t.elems.iter().enumerate() {
                if let Pat::Ident(p) = elem {
                    let value = self.element(&i.expr, n, 0);
                    self.pairs.insert(p.ident.to_string(), value);
                }
            }
            self.read_pairs.set(read);
        }
        visit::visit_local(self, l);
    }
    fn visit_expr_assign(&mut self, e: &'ast syn::ExprAssign) {
        if let Expr::Path(p) = &*e.left
            && let Some(id) = p.path.get_ident()
        {
            let value = self.resolved(&e.right, 0);
            self.aliases.insert(id.to_string(), value);
        }
        self.alpha_assign(&e.left);
        visit::visit_expr_assign(self, e);
    }
    fn visit_expr_binary(&mut self, e: &'ast syn::ExprBinary) {
        if matches!(
            e.op,
            syn::BinOp::MulAssign(_) | syn::BinOp::DivAssign(_) | syn::BinOp::SubAssign(_)
        ) {
            self.alpha_assign(&e.left);
        }
        visit::visit_expr_binary(self, e);
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let Expr::Path(p) = &*c.func
            && p.path.segments.last().is_some_and(|s| s.ident == "text")
            && c.args.len() >= 6
        {
            let color = self.resolved(&c.args[5], 0);
            self.text_color(c.func.to_token_stream().span(), &color);
        }
        visit::visit_expr_call(self, c);
    }
    fn visit_expr_struct(&mut self, s: &'ast syn::ExprStruct) {
        if s.path.segments.last().is_some_and(|p| p.ident == "TextRun") {
            for f in &s.fields {
                if f.member.to_token_stream().to_string() == "color" {
                    let c = self.resolved(&f.expr, 0);
                    self.text_color(f.expr.to_token_stream().span(), &c);
                }
            }
        }
        visit::visit_expr_struct(self, s);
    }
    fn visit_expr_method_call(&mut self, c: &'ast ExprMethodCall) {
        let name = c.method.to_string();
        let args: Vec<_> = c.args.iter().collect();
        if name == "opacity" {
            let receiver = c.receiver.to_token_stream().to_string();
            if receiver.contains("text_color") || receiver.contains("child") {
                self.hit(c, Rule::TextOpacity, "文字を含む要素に不透明度を掛けない");
            }
        }
        if name == "child"
            && self.resolved(&c.receiver, 0).contains(". bg")
            && translucent(&self.resolved(&c.receiver, 0))
        {
            self.hit(
                c,
                Rule::TranslucentTextSurface,
                "文字を置く面は不透明な面のトークンを使う",
            );
        }
        let ink_at = match name.as_str() {
            "text" | "digits" => Some((3, 4)),
            "icon" | "icon_turned" | "svg" => Some((2, 1)),
            _ => None,
        };
        if let Some((color, rect)) = ink_at
            && let (Some(color), Some(rect)) = (args.get(color), args.get(rect))
        {
            let color = self.paired(color);
            let rect = self.place(rect);
            self.fill_ink(c, &rect, &color);
        }
        if matches!(
            name.as_str(),
            "fill" | "round" | "shape" | "floating" | "ring"
        ) && let (Some(rect), Some(color)) =
            (args.first(), args.get(if name == "ring" { 2 } else { 1 }))
        {
            let color = self.paired(color);
            let rect = self.place(rect);
            if fill_face(&color) {
                self.layers.push(Layer { rect, fill: true });
            } else if ink(&color) {
                // A mark in ink (a fader's grip) on the face.
                self.fill_ink(c, &rect, &color);
            } else if !translucent(&color) {
                // A neutral face inside a fill leaves the fill as a ring round it.
                for layer in &mut self.layers {
                    if layer.fill && (rect.on(&layer.rect) || rect.inset(&layer.rect)) {
                        layer.fill = false;
                    }
                }
                self.layers.push(Layer { rect, fill: false });
            }
        }
        if matches!(name.as_str(), "text" | "digits" | "text_color") {
            if name == "text_color" && chain_opacity(&c.receiver) {
                self.hit(c, Rule::TextOpacity, "文字を含む要素に不透明度を掛けない");
            }
            let idx = if name == "text_color" { 0 } else { 3 };
            if let Some(color) = args.get(idx) {
                let s = self.resolved(color, 0);
                self.text_color(c.method.span(), &s);
                if let Some(rect) = args.get(4) {
                    let r = self.resolved(rect, 0);
                    if self
                        .faces
                        .iter()
                        .any(|f| r == *f || r.contains(&format!("{f} .")))
                    {
                        self.hit(
                            c,
                            Rule::TranslucentTextSurface,
                            "文字を置く面は不透明な面のトークンを使う",
                        );
                    }
                }
                if name == "text_color" && self.resolved(&c.receiver, 0).contains("opacity") {
                    self.hit(
                        c,
                        Rule::TranslucentTextSurface,
                        "文字を置く面は不透明な面のトークンを使う",
                    );
                }
            }
        }
        if matches!(
            name.as_str(),
            "fill" | "round" | "shape" | "floating" | "bg"
        ) {
            let idx = usize::from(name != "bg");
            if let Some(color) = args.get(idx) {
                let s = self.resolved(color, 0);
                let receiver = c.receiver.to_token_stream().to_string();
                if name == "bg"
                    && ((line(&s) && !receiver.contains("StructureLine"))
                        || track(&s)
                        || wave(&s)
                        || (contains(&s, &["fill", "Fill", "FillHover", "fill_primary"])
                            && receiver.contains("_full")))
                {
                    self.hit(c, Rule::ColorFamily, "色の族の用途外の面を作らない");
                }
                if ink(&s)
                    && name != "bg"
                    && let Some(r) = args.first()
                {
                    let rect = self.resolved(r, 0);
                    if !small_mark(&r.to_token_stream().to_string())
                        && !small_mark(&rect)
                        && self.function != "handles"
                    {
                        self.hit(c,Rule::ColorFamily,"ink は文字・アイコンだけに使う。面は surface / fill、印は line の族を使う");
                    }
                }
                if wave(&s)
                    && (!matches!(
                        self.function,
                        "paint_wave"
                            | "paint_waveform"
                            | "paint_clip"
                            | "paint_take"
                            | "paint_lanes"
                    ) || !["wave.rs", "draw.rs", "rec_area.rs", "audition.rs"]
                        .iter()
                        .any(|p| self.label.replace('\\', "/").ends_with(p)))
                {
                    self.hit(c, Rule::ExceptionFamily, "wave の族は波形だけに使う");
                }
                if alert(&s)
                    && (!matches!(
                        self.function,
                        "paint"
                            | "paint_row_parts"
                            | "paint_take"
                            | "paint_meters"
                            | "paint_head"
                            | "paint_band"
                            | "midi"
                            | "paint_meter"
                            | "meter"
                            | "paint_cell"
                            | "paint_card"
                    ) || ![
                        "rec", "export", "notice", "health", "recover", "inmeter", "input",
                        "topbar", "ruler", "paint.rs", "draw.rs", "mixer",
                    ]
                    .iter()
                    .any(|p| self.label.contains(p)))
                {
                    self.hit(
                        c,
                        Rule::ExceptionFamily,
                        "alert の族は警告・状態の印だけに使う",
                    );
                }
                if let Some(rect) = args.first() {
                    let rect = self.resolved(rect, 0);
                    if !translucent(&s) {
                        self.faces.retain(|f| f != &rect);
                    }
                }
                if translucent(&s)
                    && let Some(rect) = args.first()
                {
                    self.faces.push(self.resolved(rect, 0));
                }
                if s.contains("Ink ::") || s.contains(". ink") {
                    // Small marks and controller points may use ink; a large plane may not.
                    if name == "bg" {
                        self.hit(c, Rule::ColorFamily, "面には surface / fill の族を使う");
                    }
                }
                let path = self.label.replace('\\', "/");
                if meter(&s)
                    && (!matches!(
                        self.function,
                        "meter"
                            | "paint_meter"
                            | "paint_meters"
                            | "buffer"
                            | "audio"
                            | "midi"
                            | "paint"
                            | "paint_head"
                    ) || ![
                        "meter",
                        "paint.rs",
                        "input",
                        "inmeter",
                        "settings/panes.rs",
                        "settings/audio.rs",
                        "rec_area.rs",
                    ]
                    .iter()
                    .any(|p| path.contains(p)))
                {
                    self.hit(c, Rule::ExceptionFamily, "meter の族はメーターだけに使う");
                }
                if (s.contains(". tracks")
                    || s.contains("TrackColor")
                    || s.contains("track_ink")
                    || s.contains("color_of"))
                    && (!matches!(
                        self.function,
                        "paint"
                            | "paint_strip"
                            | "paint_row_parts"
                            | "paint_clip"
                            | "paint_mini_notes"
                            | "paint_take"
                            | "paint_lanes"
                            | "paint_cell"
                            | "paint_card"
                            | "paint_in"
                            | "sample"
                            | "lane_sample"
                            | "automation_sample"
                            | "ruler_sample"
                            | "roll"
                            | "arrange"
                            | "lane"
                            | "lanes"
                            | "lines"
                            | "wave"
                            | "overlay"
                    ) || ![
                        "draw.rs",
                        "roll/",
                        "arrange/",
                        "mixer/",
                        "popup.rs",
                        "browser",
                        "rec_area",
                        "paint.rs",
                        "export.rs",
                        "settings/mouse.rs",
                        "context/marker.rs",
                        "inspector",
                    ]
                    .iter()
                    .any(|p| path.contains(p)))
                {
                    self.hit(
                        c,
                        Rule::ExceptionFamily,
                        "track / wave の族はトラック・波形・ノートの表示だけに使う",
                    );
                }
            }
        }
        visit::visit_expr_method_call(self, c);
    }
}

use syn::spanned::Spanned;
impl Sinks<'_> {
    /// Where the rect `e` is: as written, and with its names read.
    fn place(&self, e: &Expr) -> Place {
        Place {
            written: e.to_token_stream().to_string(),
            read: self.paired(e),
        }
    }
    /// What `e` is, reading a name bound by a tuple as its element.
    fn paired(&self, e: &Expr) -> String {
        let read = self.read_pairs.replace(true);
        let value = self.resolved(e, 0);
        self.read_pairs.set(read);
        value
    }
    /// Element `i` of a pair: of each branch, of a tuple, or of the value.
    fn element(&self, e: &Expr, i: usize, depth: usize) -> String {
        if depth > 8 {
            return String::new();
        }
        let last = |b: &syn::Block| match b.stmts.last() {
            Some(syn::Stmt::Expr(e, None)) => self.element(e, i, depth + 1),
            _ => String::new(),
        };
        match e {
            Expr::Tuple(t) => t
                .elems
                .iter()
                .nth(i)
                .map_or_else(String::new, |x| self.resolved(x, depth + 1)),
            Expr::Paren(p) => self.element(&p.expr, i, depth + 1),
            Expr::Block(b) => last(&b.block),
            Expr::If(x) => {
                last(&x.then_branch)
                    + " "
                    + &x.else_branch
                        .as_ref()
                        .map_or(String::new(), |(_, e)| self.element(e, i, depth + 1))
            }
            Expr::Match(m) => m
                .arms
                .iter()
                .map(|a| self.element(&a.body, i, depth + 1))
                .collect::<Vec<_>>()
                .join(" "),
            _ => format!("{} . {i}", self.resolved(e, depth + 1)),
        }
    }
    /// Text, an icon or a mark at `rect`: on a chromatic fill (the last opaque
    /// face painted under it), its ink must be the fill's own.
    fn fill_ink(&mut self, call: &ExprMethodCall, rect: &Place, color: &str) {
        let under = self.layers.iter().rev().find(|l| rect.on(&l.rect));
        if under.is_some_and(|l| l.fill) && !fill_ink(color) {
            self.hit(
                call,
                Rule::FillInk,
                "有彩色の塗りの面の上の文字・アイコンは ink_on_fill（mute・solo の札はその対の色）を使う",
            );
        }
    }
    fn text_color(&mut self, span: proc_macro2::Span, color: &str) {
        let mut emit = |rule, message: &str| {
            let p = span.start();
            self.hits.push(Hit {
                line: p.line,
                col: p.column + 1,
                anchor: p.line,
                rule,
                message: message.into(),
            });
        };
        if translucent(color) {
            emit(Rule::TextOpacity, "文字の色に不透明度を掛けない");
        }
        if (line(color)
            || color.contains("Surface ::")
            || color_field(
                color,
                &[
                    "surface_0",
                    "surface_1",
                    "surface_2",
                    "surface_3",
                    "fill",
                    "fill_hover",
                    "danger",
                    "paper",
                    "hover",
                    "sel",
                    "clip",
                    "clip_sel",
                    "key_white",
                    "key_black",
                ],
            ))
            && !ink(color)
        {
            emit(Rule::ColorFamily, "文字には ink の族を使う");
        }
        if meter(color) || track(color) || wave(color) || alert(color) {
            emit(
                Rule::ExceptionFamily,
                "DAW の例外の色を文字に使わない。文字は ink の族を使う",
            );
        }
    }
    fn alpha_assign(&mut self, e: &Expr) {
        if let Expr::Field(f) = e
            && f.member.to_token_stream().to_string() == "a"
            && let Expr::Path(p) = &*f.base
            && let Some(id) = p.path.get_ident()
        {
            self.aliases
                .entry(id.to_string())
                .or_default()
                .push_str(" opacity");
        }
    }
}
/// An opaque face painted (UI-309): where, and whether it is a chromatic fill.
#[derive(Clone, PartialEq)]
struct Layer {
    rect: Place,
    fill: bool,
}
/// A rect as written (`cap`, `Rect::new(cap.x + 4.0, …)`) and as read through
/// the names bound before it.
#[derive(Clone, PartialEq)]
struct Place {
    written: String,
    read: String,
}
impl Place {
    /// Does this rect lie on the face `face`, as written or as read.
    fn on(&self, face: &Place) -> bool {
        inside(&self.read, &face.read) || inside(&self.written, &face.written)
    }
    /// Is this rect cut in from `face`'s edges.
    fn inset(&self, face: &Place) -> bool {
        inset(&self.read, &face.read) || inset(&self.written, &face.written)
    }
}
/// An opaque chromatic face: the primary or danger fill, or a toggle lit in one.
fn fill_face(s: &str) -> bool {
    let s = &spaced(s);
    !translucent(s)
        && (color_field(
            s,
            &[
                "fill",
                "fill_hover",
                "danger",
                "primary",
                "primary_hover",
                "arm_on",
                "monitor_on",
            ],
        ) || contains(
            s,
            &[
                "fill_primary",
                "fill_primary_hover",
                "fill_danger",
                "fill_danger_hover",
                "arm_on",
                "monitor_on",
            ],
        ) || ["Fill", "FillHover", "Danger", "DangerHover"]
            .iter()
            .any(|k| {
                s.contains(&format!("Surface :: {k} ")) || s.ends_with(&format!("Surface :: {k}"))
            }))
}
/// An ink that may stand on a fill: `ink_on_fill`, or the label of the face's pair.
fn fill_ink(s: &str) -> bool {
    contains(
        s,
        &[
            "on_fill",
            "ink_on_fill",
            "OnFill",
            "primary",
            "primary_hover",
            "arm_on",
            "monitor_on",
            "mute_on",
            "solo_on",
            "label",
        ],
    ) || s.contains(". 1")
}
/// Does the rect `rect` lie on the face `face`: the same rect, the face handed
/// to a helper (`icon_at(r)`), or a rect measured from the face's left edge.
fn inside(rect: &str, face: &str) -> bool {
    let face = &spaced(face);
    let rect = format!(" {} ", spaced(rect));
    rect.trim() == face
        || [
            format!("( {face} )"),
            format!("( {face} ,"),
            format!(", {face} )"),
            format!(", {face} ,"),
        ]
        .iter()
        .any(|p| rect.contains(p.as_str()))
        || (rect.contains(&format!(" {face} . x"))
            && !rect.contains(&format!(" {face} . right"))
            && !rect.contains(&format!(" {face} . x -")))
}
/// Is `rect` a face cut in from `face`'s edges (`Rect::new(r.x + 1.5, …)`, with
/// `r` bound to `face`): a neutral face there leaves the fill only as a ring.
fn inset(rect: &str, face: &str) -> bool {
    let (rect, face) = (spaced(rect), spaced(face));
    rect != face
        && rect.starts_with("Rect :: new (")
        && rect.contains(". x +")
        && (rect.contains(&format!("( {face} . x +"))
            || rect.split(" ) ").skip(1).any(|rest| rest.contains(&face)))
}
/// `s` with its brackets and commas as words of their own.
fn spaced(s: &str) -> String {
    s.replace('(', " ( ")
        .replace(')', " ) ")
        .replace(',', " , ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn small_mark(rect: &str) -> bool {
    if ["DOT", "POINT", "dot", "caret"]
        .iter()
        .any(|w| rect.contains(w))
    {
        return true;
    }
    let parse = |input: syn::parse::ParseStream| -> syn::Result<Expr> {
        let expr = input.parse()?;
        let _: proc_macro2::TokenStream = input.parse()?;
        Ok(expr)
    };
    if let Ok(Expr::Call(c)) = parse.parse_str(rect)
        && c.args.len() == 4
    {
        let number = |e: &Expr| match e {
            Expr::Lit(l) => match &l.lit {
                syn::Lit::Float(f) => f.base10_parse::<f32>().ok(),
                syn::Lit::Int(i) => i.base10_parse::<f32>().ok(),
                _ => None,
            },
            _ => None,
        };
        let (w, h) = (number(&c.args[2]), number(&c.args[3]));
        let dimensions = rect.contains("THUMB") || rect.contains("SQUARE");
        return dimensions
            || w.is_some_and(|w| w <= 6.0)
            || h.is_some_and(|h| h <= 6.0)
            || w.zip(h).is_some_and(|(w, h)| w <= 16.0 && h <= 16.0);
    }
    false
}
fn chain_opacity(mut e: &Expr) -> bool {
    while let Expr::MethodCall(c) = e {
        if c.method == "opacity" {
            return true;
        }
        e = &c.receiver;
    }
    false
}
