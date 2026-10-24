//! Rust のソースを歩いて、規則に当たる所を集める（コメントの例外はここでは見ない）。

use std::collections::HashSet;

use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use quote::ToTokens;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Block, Expr, ExprCall, ExprLit, ExprMethodCall, ExprStruct, Ident, ImplItemFn,
    ItemConst, ItemFn, ItemImpl, ItemMod, ItemStatic, Lit, LitStr, Local, Macro, Pat, Token,
};

use crate::rule::Rule;

/// 規則に当たった所。
#[derive(Clone, Debug)]
pub struct Hit {
    pub line: usize,
    pub col: usize,
    pub rule: Rule,
    pub message: String,
    /// メソッドの連なりが始まる行。例外の印は、この行（の次の行）にも置ける。
    pub anchor: usize,
}

/// 色を作る関数。引数の有無を問わない。
const COLOR_FNS: [&str; 6] = ["rgb", "rgba", "hsl", "hsla", "hsv", "opaque_grey"];
/// 色を作る関数。引数なしで呼ぶ名前つきの色。
const NAMED_COLOR_FNS: [&str; 8] = [
    "black",
    "white",
    "red",
    "green",
    "blue",
    "yellow",
    "transparent_black",
    "transparent_white",
];
/// 色の構造体。
const COLOR_STRUCTS: [&str; 3] = ["Hsla", "Rgba", "Color"];
/// 角丸を直に渡すメソッド。
const RADIUS_FNS: [&str; 3] = ["border_radius", "corner_radius", "corner_radii"];
/// グラデーションの語を含んでも、`xirius-design` の API なので通す名前。
const GRADIENT_ALLOWED: [&str; 3] = ["ValueGradient", "GradientStop", "gradient"];
/// これ以下の太さの固定値の面は、線とみなす（px）。
const THIN_LINE_PX: f64 = 3.0;
/// トラックの色を取り出す関数・型。
const TRACK_COLOR_SOURCES: [&str; 4] = ["track_ink", "color_of", "track_color", "TrackColor"];
/// 面を塗る `Pen` のメソッド（1 つ目が矩形、2 つ目が色）。
const PEN_FACES: [&str; 3] = ["fill", "round", "shape"];
/// これ以下の幅か高さの固定値の矩形は、端の帯とみなす（px）。
const EDGE_STRIP_PX: f64 = 6.0;
/// 文言に使わない硬い言い回しと、代わりの書き方（部分一致）。
const STIFF_WORDS: [(&str, &str); 19] = [
    ("要ります", "必要です"),
    ("が要る", "が必要です"),
    ("格子", "グリッド"),
    ("吸着", "スナップ"),
    ("寄せる", "クオンタイズ・スナップ"),
    ("入り切り", "オン・オフ"),
    ("調べ直", "再スキャン"),
    ("版の", "バージョンの"),
    ("版を", "バージョンを"),
    ("試しに実行", "ドライラン"),
    ("素通し", "バイパス"),
    ("立て直", "再起動"),
    ("進み具合", "進行状況"),
    ("名乗", "通知"),
    ("のどれか", "のいずれか"),
    ("を作った", "を作成しました"),
    ("落ちた", "クラッシュした"),
    ("落ちている", "クラッシュしている"),
    ("落とした", "クラッシュさせた"),
];
/// 画面の文字に置かない、操作を教える言い回し（部分一致。`xirius-ui` の画面の文字だけ）。
/// ユーザーの決まりは「説明・案内は置かず、アイコン・配置・状態で伝える」。エラーの理由と、
/// 取り返しのつかない操作の確認の文は残してよい（理由を付けて allow する）。
const GUIDE_PHRASES: [&str; 10] = [
    "ください",
    "クリックで",
    "ダブルクリック",
    "右クリック",
    "ドラッグで",
    "持ち替え",
    "押すと",
    "すると",
    "ここに並び",
    "ここに表示",
];
/// 使ってはいけない書体名（完全一致）。
const FONT_NAMES: [&str; 29] = [
    "Inter",
    "Noto Sans JP",
    "Noto Sans",
    "Noto Serif JP",
    "Helvetica",
    "Helvetica Neue",
    "Arial",
    "Roboto",
    "Segoe UI",
    "SF Pro",
    "SF Pro Text",
    "SF Pro Display",
    "San Francisco",
    ".SystemUIFont",
    "Menlo",
    "Monaco",
    "Consolas",
    "Courier New",
    "Meiryo",
    "Yu Gothic",
    "Hiragino Sans",
    "Hiragino Kaku Gothic ProN",
    "MS Gothic",
    "Apple Color Emoji",
    "Apple Symbols",
    "Noto Color Emoji",
    "system-ui",
    "sans-serif",
    "monospace",
];
/// 使ってはいけない書体名の頭。Windows の Segoe の一族（`Segoe Fluent Icons`・`Segoe MDL2 Assets`・
/// `Segoe UI Symbol`・`Segoe UI Emoji` ほか）は、窓のボタンや記号に使われやすい。
const FONT_PREFIXES: [&str; 1] = ["Segoe "];

/// `screen` は、このファイルの文字列が画面に出る文字か（案内の文の規則を当てるか）。
pub fn scan(file: &syn::File, screen: bool) -> Vec<Hit> {
    let mut scanner = Scanner {
        screen,
        ..Scanner::default()
    };
    scanner.visit_file(file);
    let mut hits = scanner.hits;
    hits.sort_by_key(|h| (h.line, h.col, h.rule));
    hits.dedup_by_key(|h| (h.line, h.col, h.rule));
    hits
}

#[derive(Default)]
struct Scanner {
    screen: bool,
    hits: Vec<Hit>,
    /// 関数ごとの、トラックの色を持つ変数と、細い矩形の変数。
    locals: Vec<(HashSet<String>, HashSet<String>)>,
}

impl Scanner {
    fn hit(&mut self, span: Span, rule: Rule, message: impl Into<String>, anchor: Option<usize>) {
        let start = span.start();
        self.hits.push(Hit {
            line: start.line,
            col: start.column + 1,
            rule,
            message: message.into(),
            anchor: anchor.unwrap_or(start.line),
        });
    }

    fn check_gradient(&mut self, ident: &Ident, anchor: Option<usize>) {
        let name = ident.to_string();
        if name.to_lowercase().contains("gradient") && !GRADIENT_ALLOWED.contains(&name.as_str()) {
            self.hit(
                ident.span(),
                Rule::Gradient,
                format!(
                    "`{name}`: グラデーションは値を表す物（メーター）だけ。面は単色にする（xirius_design::LevelMeter）"
                ),
                anchor,
            );
        }
    }

    fn check_str(&mut self, value: &str, span: Span) {
        for (stiff, better) in STIFF_WORDS {
            if value.contains(stiff) {
                self.hit(
                    span,
                    Rule::Wording,
                    format!("「{stiff}」: 硬い言い回し。「{better}」のように書く"),
                    None,
                );
            }
        }
        if self.screen {
            for phrase in GUIDE_PHRASES {
                if value.contains(phrase) {
                    self.hit(
                        span,
                        Rule::Guidance,
                        format!(
                            "「{phrase}」: 操作を教える文。画面に説明・案内を置かず、アイコン・配置・状態で伝える（エラーの理由と確認の文は理由を付けて allow する）"
                        ),
                        None,
                    );
                }
            }
        }
        if is_hex_color(value) || is_css_color_fn(value) {
            self.hit(
                span,
                Rule::RawColor,
                format!("\"{value}\": 生の色。xirius_design の Surface・Ink・Accent・TrackColor から取る"),
                None,
            );
        }
        if FONT_NAMES.contains(&value) || FONT_PREFIXES.iter().any(|p| value.starts_with(p)) {
            self.hit(
                span,
                Rule::RawFont,
                format!("\"{value}\": 書体名の文字列。xirius_design::Font から取る"),
                None,
            );
        }
    }

    fn check_color_call(&mut self, name: &str, prefix: Option<&str>, args_empty: bool, span: Span) {
        let hex_ctor = prefix == Some("Color") && matches!(name, "hex" | "hex_alpha");
        let named = NAMED_COLOR_FNS.contains(&name) && args_empty;
        if COLOR_FNS.contains(&name) || named || hex_ctor {
            self.hit(
                span,
                Rule::RawColor,
                format!("`{name}(…)`: 生の色。xirius_design の Surface・Ink・Accent・TrackColor から取る"),
                None,
            );
        }
    }

    /// メソッド 1 つ分の検査。`has_literal` は引数に数値の直書きがあるか。
    fn check_method(&mut self, method: &Ident, has_literal: bool, args_empty: bool, anchor: usize) {
        self.check_gradient(method, Some(anchor));
        let name = method.to_string();
        let span = method.span();

        if RADIUS_FNS.contains(&name.as_str()) {
            if has_literal {
                self.hit(
                    span,
                    Rule::RawRadius,
                    format!("`.{name}(…)`: 生の角丸。xirius_design::Radius から取る"),
                    Some(anchor),
                );
            }
        } else if name == "border" || name.starts_with("border_") {
            self.hit(
                span,
                Rule::Border,
                format!("`.{name}(…)`: 線は使わない。面の差と余白で分ける"),
                Some(anchor),
            );
        } else if name == "rounded" {
            if has_literal {
                self.hit(
                    span,
                    Rule::RawRadius,
                    "`.rounded(…)`: 生の角丸。xirius_design::Radius から取る",
                    Some(anchor),
                );
            }
        } else if name.starts_with("rounded_")
            && name != "rounded_none"
            && (args_empty || has_literal)
        {
            self.hit(
                span,
                Rule::RawRadius,
                format!("`.{name}(…)`: 既定値か生の角丸。xirius_design::Radius から取る"),
                Some(anchor),
            );
        }
    }

    /// 検査の通る形に解けなかったマクロの中身を、トークンのまま探す。
    fn scan_tokens(&mut self, stream: TokenStream, anchor: Option<usize>) {
        let tokens: Vec<TokenTree> = stream.into_iter().collect();
        for (i, tree) in tokens.iter().enumerate() {
            match tree {
                TokenTree::Ident(id) => {
                    let name = id.to_string();
                    let args = match tokens.get(i + 1) {
                        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => {
                            Some(g.stream())
                        }
                        _ => None,
                    };
                    let after_dot = i > 0
                        && matches!(&tokens[i - 1], TokenTree::Punct(p) if p.as_char() == '.');
                    self.check_gradient(id, anchor);
                    if let Some(args) = &args {
                        let args_empty = args.is_empty();
                        if after_dot {
                            let has_literal = has_numeric_literal(args.clone());
                            self.check_method(id, has_literal, args_empty, id.span().start().line);
                        } else {
                            self.check_color_call(&name, None, args_empty, id.span());
                        }
                    } else if after_dot {
                        self.check_method(id, false, true, id.span().start().line);
                    }
                }
                TokenTree::Literal(l) => {
                    if let Some(value) = unquote(&l.to_string()) {
                        self.check_str(&value, l.span());
                    }
                }
                TokenTree::Group(g) => self.scan_tokens(g.stream(), anchor),
                TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scanner {
    fn visit_attribute(&mut self, _: &'ast Attribute) {}

    fn visit_item_mod(&mut self, i: &'ast ItemMod) {
        if !is_test_only(&i.attrs) {
            visit::visit_item_mod(self, i);
        }
    }

    fn visit_item_fn(&mut self, i: &'ast ItemFn) {
        if !is_test_only(&i.attrs) {
            self.locals.push(fn_locals(&i.block));
            visit::visit_item_fn(self, i);
            self.locals.pop();
        }
    }

    fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
        if !is_test_only(&i.attrs) {
            visit::visit_item_impl(self, i);
        }
    }

    fn visit_impl_item_fn(&mut self, i: &'ast ImplItemFn) {
        if !is_test_only(&i.attrs) {
            self.locals.push(fn_locals(&i.block));
            visit::visit_impl_item_fn(self, i);
            self.locals.pop();
        }
    }

    fn visit_item_const(&mut self, i: &'ast ItemConst) {
        if !is_test_only(&i.attrs) {
            visit::visit_item_const(self, i);
        }
    }

    fn visit_item_static(&mut self, i: &'ast ItemStatic) {
        if !is_test_only(&i.attrs) {
            visit::visit_item_static(self, i);
        }
    }

    fn visit_ident(&mut self, i: &'ast Ident) {
        self.check_gradient(i, None);
    }

    fn visit_lit_str(&mut self, l: &'ast LitStr) {
        self.check_str(&l.value(), l.span());
    }

    fn visit_expr_call(&mut self, c: &'ast ExprCall) {
        if let Expr::Path(p) = &*c.func {
            let segments: Vec<String> = p
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            if let Some((name, rest)) = segments.split_last() {
                let prefix = rest.last().map(String::as_str);
                let span = p
                    .path
                    .segments
                    .last()
                    .map_or_else(Span::call_site, |s| s.ident.span());
                self.check_color_call(name, prefix, c.args.is_empty(), span);
            }
        }
        visit::visit_expr_call(self, c);
    }

    fn visit_expr_struct(&mut self, s: &'ast ExprStruct) {
        if let Some(last) = s.path.segments.last() {
            let name = last.ident.to_string();
            if COLOR_STRUCTS.contains(&name.as_str()) {
                self.hit(
                    last.ident.span(),
                    Rule::RawColor,
                    format!("`{name} {{ … }}`: 生の色。xirius_design の Surface・Ink・Accent・TrackColor から取る"),
                    None,
                );
            }
        }
        self.check_box_shadow(s);
        visit::visit_expr_struct(self, s);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        // `a.b().c().d()` を 1 本の連なりとして見る。
        let mut chain: Vec<&ExprMethodCall> = vec![call];
        let mut base: &Expr = &call.receiver;
        while let Expr::MethodCall(inner) = base {
            chain.push(inner);
            base = &inner.receiver;
        }
        chain.reverse();
        let anchor = first_span(base).start().line;

        for c in &chain {
            let has_literal = c
                .args
                .iter()
                .any(|a| has_numeric_literal(a.to_token_stream()));
            self.check_method(&c.method, has_literal, c.args.is_empty(), anchor);
            if c.method == "font_family" {
                for arg in &c.args {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = arg
                    {
                        self.hit(
                            s.span(),
                            Rule::RawFont,
                            format!("`.font_family(\"{}\")`: 書体名の文字列。xirius_design::Font から取る", s.value()),
                            Some(anchor),
                        );
                    }
                }
            }
        }
        self.check_edges(&chain, anchor);
        self.check_line(&chain, anchor);
        self.check_track_strip(&chain, anchor);

        for c in &chain {
            for arg in &c.args {
                self.visit_expr(arg);
            }
        }
        self.visit_expr(base);
    }

    fn visit_macro(&mut self, m: &'ast Macro) {
        let parsed = Punctuated::<Expr, Token![,]>::parse_terminated.parse2(m.tokens.clone());
        match parsed {
            Ok(args) => {
                for arg in &args {
                    self.visit_expr(arg);
                }
            }
            Err(_) => self.scan_tokens(m.tokens.clone(), None),
        }
    }
}

impl Scanner {
    /// 細い線。`bg` を持ち、`w`・`h` の固定値が `THIN_LINE_PX` 以下の面。
    /// 太さ・長さの引数に `StructureLine`・`GridLine` が出る呼び出し
    /// （`.w(px(StructureLine::THICKNESS))`）は、数えない。その引数に数が混ざっていても（`StructureLine::END_INSET * 2.0`）通す。
    /// 端に貼り付いた帯は `one-sided-edge` が先に止めるので、ここでは数えない。
    fn check_line(&mut self, chain: &[&ExprMethodCall], anchor: usize) {
        let has = |n: &str| chain.iter().any(|c| c.method == n);
        if !(has("bg") || has("background")) {
            return;
        }
        let thin = chain.iter().any(|c| {
            let through_api = c.args.iter().any(|a| {
                tokens_have_ident(a.to_token_stream(), "StructureLine")
                    || tokens_have_ident(a.to_token_stream(), "GridLine")
            });
            matches!(c.method.to_string().as_str(), "w" | "h" | "w_px" | "h_px")
                && !through_api
                && smallest_literal(&c.args).is_some_and(|v| v <= THIN_LINE_PX)
        });
        if !thin {
            return;
        }
        let pinned = ["left_0", "right_0", "top_0", "bottom_0"]
            .iter()
            .filter(|n| has(n))
            .count();
        if has("absolute") && pinned == 1 {
            return;
        }
        if let Some(c) = chain
            .iter()
            .find(|c| c.method == "bg" || c.method == "background")
        {
            self.hit(
                c.method.span(),
                Rule::Line,
                "細い線（太さが 3px 以下の固定値の面）。線で区切らない。                     フォルダーの範囲を示す線なら、太さと位置を xirius_design::StructureLine から取る。                     時間のグリッドの線なら xirius_design::GridLine から取る",
                Some(anchor),
            );
        }
    }

    /// 片側にだけ出る影・光（`BoxShadow { offset: point(0, 1), blur_radius: 0, … }`）。
    /// ぼかしが無く、広がりも無く、ずらしが縦か横の片方だけの物。色が中立でも違反。
    /// ぼかしのある全周の外側の影（浮く面の影）は通す。
    fn check_box_shadow(&mut self, s: &ExprStruct) {
        if s.path
            .segments
            .last()
            .is_none_or(|l| l.ident != "BoxShadow")
        {
            return;
        }
        let field = |name: &str| s.fields.iter().find(|f| member_is(&f.member, name));
        let zero = |name: &str| {
            field(name).is_none_or(|f| {
                let mut values = Vec::new();
                numeric_literals(f.expr.to_token_stream(), &mut values);
                values.iter().all(|v| *v == 0.0)
            })
        };
        let Some(offset) = field("offset") else {
            return;
        };
        let mut axes = Vec::new();
        if let Expr::Call(call) = &offset.expr {
            for arg in &call.args {
                let mut values = Vec::new();
                numeric_literals(arg.to_token_stream(), &mut values);
                axes.push(values.iter().any(|v| *v != 0.0));
            }
        }
        if axes.len() == 2
            && axes[0] != axes[1]
            && zero("blur_radius")
            && zero("spread_radius")
            && let Some(last) = s.path.segments.last()
        {
            self.hit(
                last.ident.span(),
                Rule::OneSidedEdge,
                "片側だけに出る影・光（ぼかしなしで縦か横の一方にずらした影）。                     面の区切りは面の明るさの差と、全周の外側の影だけで作る",
                None,
            );
        }
    }

    /// `Pen` で、トラックの色を細い矩形に塗る所（レーンや頭の端の色の帯）。
    /// トラックの色は線と点にだけ使い、面の端に帯として付けない（AUT-014）。
    fn check_track_strip(&mut self, chain: &[&ExprMethodCall], anchor: usize) {
        let Some((colored, thin)) = self.locals.last() else {
            return;
        };
        let mut found = Vec::new();
        for c in chain {
            if !PEN_FACES.contains(&c.method.to_string().as_str()) || c.args.len() < 2 {
                continue;
            }
            let rect = &c.args[0];
            let color = c.args[1].to_token_stream();
            let strip = is_thin_rect(rect) || mentions(rect.to_token_stream(), thin);
            let track = is_track_color(color.clone()) || mentions(color, colored);
            if strip && track {
                found.push(c.method.span());
            }
        }
        for span in found {
            self.hit(
                span,
                Rule::OneSidedEdge,
                "トラックの色の細い帯（面の端の色の帯）。トラックの色は線と点にだけ使う",
                Some(anchor),
            );
        }
    }

    /// 角丸の物の 1 辺の色。色が中立（面の上辺の光など）でも、1 辺だけなら違反。
    fn check_edges(&mut self, chain: &[&ExprMethodCall], anchor: usize) {
        let names: Vec<String> = chain.iter().map(|c| c.method.to_string()).collect();
        let has = |n: &str| names.iter().any(|x| x == n);

        let rounded = names
            .iter()
            .any(|n| n.starts_with("rounded") && n != "rounded_none");
        if rounded {
            for c in chain {
                if is_one_sided_border(&c.method.to_string()) {
                    self.hit(
                        c.method.span(),
                        Rule::OneSidedEdge,
                        format!("`.{}(…)`: 角丸の物の 1 辺だけに色を付けない", c.method),
                        Some(anchor),
                    );
                }
            }
        }

        // 端に貼り付いた細い色の帯（左端の色の帯など）。
        let pinned = ["left_0", "right_0", "top_0", "bottom_0"]
            .iter()
            .filter(|n| has(n))
            .count();
        let thin = chain.iter().any(|c| {
            let n = c.method.to_string();
            match n.as_str() {
                "w" | "h" => smallest_literal(&c.args).is_some_and(|v| v <= 6.0),
                "w_px" | "h_px" => true,
                _ => false,
            }
        });
        if has("absolute")
            && (has("bg") || has("background"))
            && pinned == 1
            && thin
            && let Some(c) = chain
                .iter()
                .find(|c| c.method == "bg" || c.method == "background")
        {
            self.hit(
                c.method.span(),
                Rule::OneSidedEdge,
                "端に貼り付いた細い色の帯（1 辺だけの色）。面の端に色を付けない。\
                     スクロールのつまみ・再生位置の線なら、理由を付けて allow する",
                Some(anchor),
            );
        }
    }
}

/// 関数の中の `let` のうち、トラックの色を持つ名前と、細い矩形の名前。
fn fn_locals(block: &Block) -> (HashSet<String>, HashSet<String>) {
    #[derive(Default)]
    struct Lets<'a>(Vec<(String, &'a Expr)>);
    impl<'a> Visit<'a> for Lets<'a> {
        fn visit_local(&mut self, l: &'a Local) {
            let name = match &l.pat {
                Pat::Ident(p) => Some(p.ident.to_string()),
                Pat::Type(t) => match &*t.pat {
                    Pat::Ident(p) => Some(p.ident.to_string()),
                    _ => None,
                },
                _ => None,
            };
            if let (Some(name), Some(init)) = (name, &l.init) {
                self.0.push((name, &init.expr));
            }
            visit::visit_local(self, l);
        }
    }
    let mut lets = Lets::default();
    lets.visit_block(block);
    // 色は別の変数を通って渡ることがあるので、増えなくなるまで回す。同じ名前を
    // 使い直したものは、どの束縛も当たる（自分を元にした束縛は続きとみなす）とき
    // だけ数える。中立の色に使い直した名前は数えない。
    let mut colored = HashSet::new();
    let mut thin = HashSet::new();
    loop {
        let before = (colored.len(), thin.len());
        colored = settle(&lets.0, &colored, |e, set| {
            let t = e.to_token_stream();
            is_track_color(t.clone()) || mentions(t, set)
        });
        thin = settle(&lets.0, &thin, |e, set| {
            is_thin_rect(e) || mentions(e.to_token_stream(), set)
        });
        if (colored.len(), thin.len()) == before {
            return (colored, thin);
        }
    }
}

/// `lets` の名前のうち、どの束縛も `hit` に当たり、少なくとも 1 つは自分の名前に
/// 頼らずに当たるもの。
fn settle(
    lets: &[(String, &Expr)],
    set: &HashSet<String>,
    hit: impl Fn(&Expr, &HashSet<String>) -> bool,
) -> HashSet<String> {
    let names: HashSet<&String> = lets.iter().map(|(n, _)| n).collect();
    names
        .into_iter()
        .filter(|n| {
            let mut with_self = set.clone();
            with_self.insert((*n).clone());
            let mine: Vec<&Expr> = lets
                .iter()
                .filter(|(m, _)| m == *n)
                .map(|(_, e)| *e)
                .collect();
            mine.iter().all(|e| hit(e, &with_self)) && mine.iter().any(|e| hit(e, set))
        })
        .cloned()
        .collect()
}

/// トラックの色を取り出す式（`track_ink(…)`・`c.tracks[i]`・`TrackColor`）。
fn is_track_color(stream: TokenStream) -> bool {
    let mut tokens = stream.into_iter().peekable();
    while let Some(t) = tokens.next() {
        match t {
            TokenTree::Ident(i) => {
                let name = i.to_string();
                if TRACK_COLOR_SOURCES.contains(&name.as_str()) {
                    return true;
                }
                if name == "tracks"
                    && matches!(tokens.peek(), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket)
                {
                    return true;
                }
            }
            TokenTree::Group(g) if is_track_color(g.stream()) => return true,
            _ => {}
        }
    }
    false
}

/// `set` の名前が出る式。
fn mentions(stream: TokenStream, set: &HashSet<String>) -> bool {
    !set.is_empty()
        && stream.into_iter().any(|t| match t {
            TokenTree::Ident(i) => set.contains(&i.to_string()),
            TokenTree::Group(g) => mentions(g.stream(), set),
            _ => false,
        })
}

/// `Rect::new(x, y, w, h)` で、幅か高さの一方だけが `EDGE_STRIP_PX` 以下の固定値
/// （両方が小さいものは点）。
fn is_thin_rect(expr: &Expr) -> bool {
    let Expr::Call(call) = expr else {
        return false;
    };
    let Expr::Path(p) = &*call.func else {
        return false;
    };
    let names: Vec<String> = p
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    if !names.ends_with(&["Rect".to_string(), "new".to_string()]) || call.args.len() != 4 {
        return false;
    }
    let small = |a: &Expr| match a {
        Expr::Lit(ExprLit {
            lit: Lit::Float(f), ..
        }) => f.base10_parse::<f64>().is_ok_and(|v| v <= EDGE_STRIP_PX),
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => i.base10_parse::<f64>().is_ok_and(|v| v <= EDGE_STRIP_PX),
        _ => false,
    };
    small(&call.args[2]) != small(&call.args[3])
}

fn member_is(member: &syn::Member, name: &str) -> bool {
    matches!(member, syn::Member::Named(id) if id == name)
}

fn first_span(expr: &Expr) -> Span {
    expr.to_token_stream()
        .into_iter()
        .next()
        .map_or_else(Span::call_site, |t| t.span())
}

fn tokens_have_ident(stream: TokenStream, ident: &str) -> bool {
    stream.into_iter().any(|t| match t {
        TokenTree::Ident(i) => i == ident,
        TokenTree::Group(g) => tokens_have_ident(g.stream(), ident),
        _ => false,
    })
}

fn has_numeric_literal(stream: TokenStream) -> bool {
    stream.into_iter().any(|t| match t {
        TokenTree::Literal(l) => l.to_string().starts_with(|c: char| c.is_ascii_digit()),
        TokenTree::Group(g) => has_numeric_literal(g.stream()),
        _ => false,
    })
}

fn numeric_literals(stream: TokenStream, out: &mut Vec<f64>) {
    for t in stream {
        match t {
            TokenTree::Literal(l) => {
                let s = l.to_string();
                if s.starts_with(|c: char| c.is_ascii_digit()) {
                    let number: String = s
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '_')
                        .filter(|c| *c != '_')
                        .collect();
                    if let Ok(v) = number.parse::<f64>() {
                        out.push(v);
                    }
                }
            }
            TokenTree::Group(g) => numeric_literals(g.stream(), out),
            _ => {}
        }
    }
}

fn smallest_literal(args: &Punctuated<Expr, Token![,]>) -> Option<f64> {
    let mut values = Vec::new();
    for a in args {
        numeric_literals(a.to_token_stream(), &mut values);
    }
    values.into_iter().reduce(f64::min)
}

/// `border_t`・`border_l_1` のように、4 辺のうち 1 辺だけを指す名前。
fn is_one_sided_border(name: &str) -> bool {
    name.strip_prefix("border_")
        .and_then(|rest| rest.split('_').next())
        .is_some_and(|side| matches!(side, "t" | "b" | "l" | "r"))
}

pub(crate) fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if a.path().is_ident("cfg")
            && let Ok(list) = a.meta.require_list()
        {
            let tokens = list.tokens.clone();
            return tokens_have_ident(tokens.clone(), "test") && !tokens_have_ident(tokens, "not");
        }
        false
    })
}

fn is_hex_color(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_css_color_fn(s: &str) -> bool {
    let s = s.trim_start();
    ["rgb(", "rgba(", "hsl(", "hsla(", "oklch(", "color-mix("]
        .iter()
        .any(|p| s.starts_with(p))
}

/// 文字列のリテラル（`"…"`・`r"…"`・`r#"…"#`）の中身。文字列でなければ `None`。
fn unquote(literal: &str) -> Option<String> {
    let rest = literal.strip_prefix('r').unwrap_or(literal);
    let rest = rest.trim_start_matches('#');
    let rest = rest.strip_prefix('"')?;
    let end = rest.rfind('"')?;
    Some(rest[..end].to_string())
}
