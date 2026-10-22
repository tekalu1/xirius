//! モック（HTML の `<style>`）の CSS に、同じ種類の規則を当てて数える。
//! 数えるだけで、モックは直さない（何が残っているかを、実装の前に知るため）。
//!
//! 数える物:
//! - `gradient`: グラデーション。メーターの値の帯は「除外」に分ける。
//! - `border`: 線（`border*`・`outline`）。フォーカスの輪は「除外」。
//! - `tint`: 文字の下に敷く薄い色の面（半透明の背景・`color-mix`・`*-tint`）。幕・影は「除外」。
//! - `one-sided`: 1 辺だけの色（片側の線・片側だけの `box-shadow`・端に貼り付いた細い帯）。
//!   中立の白・黒の光（面の上辺の光）も同じ違反。ぼかしのある全周の外側の影は数えない。

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CssRule {
    Gradient,
    Border,
    Tint,
    OneSided,
}

impl CssRule {
    pub const ALL: [CssRule; 4] = [
        CssRule::Gradient,
        CssRule::Border,
        CssRule::Tint,
        CssRule::OneSided,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            CssRule::Gradient => "gradient",
            CssRule::Border => "border",
            CssRule::Tint => "tint",
            CssRule::OneSided => "one-sided",
        }
    }
}

impl fmt::Display for CssRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Violation,
    /// 規則の対象外とみなしたもの（理由つき）。
    Excluded(&'static str),
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub rule: CssRule,
    pub line: usize,
    pub selector: String,
    pub text: String,
    pub kind: Kind,
}

#[derive(Debug, Default)]
pub struct CssReport {
    pub findings: Vec<Finding>,
    pub declarations: usize,
    /// `style="…"`（属性）の数。ここは数えていない。
    pub inline_styles: usize,
    /// 定義されている `*-tint` のトークンの数。
    pub tint_tokens: usize,
}

impl CssReport {
    pub fn violations(&self, rule: CssRule) -> usize {
        self.count(rule, |k| *k == Kind::Violation)
    }

    pub fn excluded(&self, rule: CssRule) -> usize {
        self.count(rule, |k| matches!(k, Kind::Excluded(_)))
    }

    fn count(&self, rule: CssRule, f: impl Fn(&Kind) -> bool) -> usize {
        self.findings
            .iter()
            .filter(|x| x.rule == rule && f(&x.kind))
            .count()
    }

    pub fn total_violations(&self) -> usize {
        CssRule::ALL.iter().map(|r| self.violations(*r)).sum()
    }
}

/// HTML（または CSS）の文字列を検査する。
pub fn check(html: &str) -> CssReport {
    let mut report = CssReport {
        inline_styles: html.matches("style=\"").count(),
        ..CssReport::default()
    };

    for (css, first_line) in style_blocks(html) {
        let decls = declarations(&css, first_line);
        report.declarations += decls.len();
        report.tint_tokens += decls
            .iter()
            .filter(|d| d.prop.starts_with("--") && d.prop.ends_with("-tint"))
            .count();
        check_decls(&decls, &mut report.findings);
    }
    report.findings.sort_by_key(|a| (a.rule, a.line));
    report
}

/// `<style>` の中身と、その先頭の行番号。`<style>` が無ければ全体を CSS とみなす。
fn style_blocks(html: &str) -> Vec<(String, usize)> {
    let lower = html.to_lowercase();
    if !lower.contains("<style") {
        return vec![(html.to_string(), 1)];
    }
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(open) = lower[from..].find("<style") {
        let open = from + open;
        let Some(gt) = lower[open..].find('>') else {
            break;
        };
        let start = open + gt + 1;
        let end = lower[start..]
            .find("</style>")
            .map_or(html.len(), |e| start + e);
        let line = html[..start].matches('\n').count() + 1;
        blocks.push((html[start..end].to_string(), line));
        from = end;
    }
    blocks
}

#[derive(Clone, Debug)]
struct Decl {
    block: usize,
    selector: String,
    prop: String,
    value: String,
    line: usize,
}

fn declarations(css: &str, first_line: usize) -> Vec<Decl> {
    let chars: Vec<char> = css.chars().collect();
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut buffer = String::new();
    let mut buffer_line = first_line;
    let mut line = first_line;
    let mut paren = 0usize;
    let mut quote: Option<char> = None;
    let mut block = 0usize;
    let mut block_ids: Vec<usize> = Vec::new();
    let mut i = 0;

    let emit = |buffer: &str, line: usize, stack: &[String], block: usize, out: &mut Vec<Decl>| {
        let Some((prop, value)) = buffer.split_once(':') else {
            return;
        };
        if stack
            .iter()
            .any(|h| h.starts_with("@keyframes") || h.starts_with("@font-face"))
        {
            return;
        }
        let selector = stack
            .iter()
            .rev()
            .find(|h| !h.starts_with('@'))
            .cloned()
            .unwrap_or_default();
        out.push(Decl {
            block,
            selector,
            prop: prop.trim().to_lowercase(),
            value: value.trim().to_string(),
            line,
        });
    };

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
        }
        if quote.is_none() && c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        if let Some(q) = quote {
            buffer.push(c);
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                buffer.push(c);
            }
            '(' => {
                paren += 1;
                buffer.push(c);
            }
            ')' => {
                paren = paren.saturating_sub(1);
                buffer.push(c);
            }
            '{' if paren == 0 => {
                stack.push(buffer.trim().to_string());
                block += 1;
                block_ids.push(block);
                buffer.clear();
            }
            '}' if paren == 0 => {
                if !buffer.trim().is_empty() {
                    emit(
                        &buffer,
                        buffer_line,
                        &stack,
                        *block_ids.last().unwrap_or(&0),
                        &mut out,
                    );
                }
                stack.pop();
                block_ids.pop();
                buffer.clear();
            }
            ';' if paren == 0 => {
                emit(
                    &buffer,
                    buffer_line,
                    &stack,
                    *block_ids.last().unwrap_or(&0),
                    &mut out,
                );
                buffer.clear();
            }
            _ => {
                if buffer.trim().is_empty() && !c.is_whitespace() {
                    buffer_line = line;
                }
                buffer.push(c);
            }
        }
        i += 1;
    }
    out
}

fn push(findings: &mut Vec<Finding>, rule: CssRule, d: &Decl, kind: Kind) {
    findings.push(Finding {
        rule,
        line: d.line,
        selector: d.selector.clone(),
        text: format!("{}: {}", d.prop, d.value),
        kind,
    });
}

fn check_decls(decls: &[Decl], findings: &mut Vec<Finding>) {
    for d in decls {
        let selector = d.selector.to_lowercase();
        let value = d.value.to_lowercase();

        // グラデーション
        if value.contains("gradient(") {
            let kind = if ["meter", "mtr", "gauge", "level", "vu", "peak"]
                .iter()
                .any(|w| selector.contains(w))
            {
                Kind::Excluded("値を表す帯（メーター）")
            } else if value.contains("transparent") && value.contains("1px") {
                Kind::Excluded("線を引く手法（色の変化ではない）")
            } else {
                Kind::Violation
            };
            push(findings, CssRule::Gradient, d, kind);
        }

        // 線
        let is_side_border = ["border-left", "border-right", "border-top", "border-bottom"]
            .iter()
            .any(|p| d.prop == *p || d.prop.starts_with(&format!("{p}-")));
        let is_border = d.prop == "border"
            || (d.prop.starts_with("border-")
                && !matches!(
                    d.prop.as_str(),
                    "border-radius" | "border-collapse" | "border-spacing"
                )
                && !d.prop.contains("radius"));
        if is_border && !is_none(&value) && !d.prop.starts_with("border-color") {
            if is_side_border {
                push(findings, CssRule::OneSided, d, Kind::Violation);
            } else if value.contains("transparent") {
                push(
                    findings,
                    CssRule::Border,
                    d,
                    Kind::Excluded("透明の枠（余白・三角の作り方。色は見えない）"),
                );
            } else {
                push(findings, CssRule::Border, d, Kind::Violation);
            }
        }
        if (d.prop == "outline" || d.prop == "outline-color") && !is_none(&value) {
            let kind = if selector.contains(":focus") {
                Kind::Excluded("フォーカスの輪")
            } else {
                Kind::Violation
            };
            push(findings, CssRule::Border, d, kind);
        }

        // 薄い色の面
        if (d.prop == "background" || d.prop == "background-color") && is_tint(&value) {
            let kind = if ["veil", "scrim", "overlay", "backdrop", "shadow"]
                .iter()
                .any(|w| selector.contains(w) || value.contains(w))
            {
                Kind::Excluded("画面を覆う幕")
            } else {
                Kind::Violation
            };
            push(findings, CssRule::Tint, d, kind);
        }

        // 片側だけの影
        if d.prop == "box-shadow" {
            for shadow in split_top_level(&d.value, ',') {
                if let Some(kind) = one_sided_shadow(&shadow) {
                    push(findings, CssRule::OneSided, d, kind);
                }
            }
        }
    }

    // 端に貼り付いた細い色の帯
    let mut start = 0;
    while start < decls.len() {
        let id = decls[start].block;
        let end = decls[start..]
            .iter()
            .position(|d| d.block != id)
            .map_or(decls.len(), |n| start + n);
        if let Some(strip) = edge_strip(&decls[start..end]) {
            push(findings, CssRule::OneSided, &strip, Kind::Violation);
        }
        start = end;
    }
}

fn is_none(value: &str) -> bool {
    let v = value.trim();
    matches!(
        v,
        "0" | "none" | "0 none" | "0px" | "medium none" | "transparent" | "initial" | "inherit"
    ) || v.starts_with("0 none")
        || v.starts_with("none")
}

fn is_tint(value: &str) -> bool {
    if value.contains("color-mix(") || (value.contains("var(--") && value.contains("-tint")) {
        return true;
    }
    for func in ["rgba(", "hsla("] {
        if let Some(at) = value.find(func) {
            let inner = &value[at + func.len()..];
            let inner = inner.split(')').next().unwrap_or("");
            let alpha = inner.rsplit([',', '/']).next().unwrap_or("").trim();
            if let Ok(a) = alpha.trim_end_matches('%').parse::<f64>() {
                let a = if alpha.ends_with('%') { a / 100.0 } else { a };
                return a < 1.0;
            }
        }
    }
    false
}

fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            c if c == sep && depth == 0 => parts.push(std::mem::take(&mut cur)),
            c if c.is_whitespace() && sep == ' ' && depth == 0 => {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

fn length(token: &str) -> Option<f64> {
    token
        .strip_suffix("px")
        .unwrap_or(token)
        .parse::<f64>()
        .ok()
}

/// 影 1 つが、1 辺だけに出る物（ぼかしも広がりも無く、縦か横の一方にずらした影）なら違反。
/// 色が中立でも同じ。ぼかしのある全周の外側の影（浮く面の影）は対象にしない。
fn one_sided_shadow(shadow: &str) -> Option<Kind> {
    let tokens: Vec<String> = split_top_level(shadow.trim(), ' ');
    let mut lengths = Vec::new();
    for t in &tokens {
        if t == "inset" {
            continue;
        }
        if let Some(v) = length(t) {
            lengths.push(v);
        }
    }
    if lengths.len() < 2 {
        return None;
    }
    let (x, y) = (lengths[0], lengths[1]);
    let blur = lengths.get(2).copied().unwrap_or(0.0);
    let spread = lengths.get(3).copied().unwrap_or(0.0);
    if blur != 0.0 || spread != 0.0 || ((x != 0.0) == (y != 0.0)) {
        return None;
    }
    Some(Kind::Violation)
}

/// 絶対配置で、1 つの辺に貼り付いた、細い色の帯。
fn edge_strip(block: &[Decl]) -> Option<Decl> {
    let get = |p: &str| {
        block
            .iter()
            .find(|d| d.prop == p)
            .map(|d| d.value.to_lowercase())
    };
    if get("position")? != "absolute" {
        return None;
    }
    let background = block
        .iter()
        .find(|d| d.prop == "background" || d.prop == "background-color")?;
    let bg = background.value.to_lowercase();
    if is_none(&bg) || bg.contains("gradient") {
        return None;
    }
    let zero = |v: &str| matches!(v.trim(), "0" | "0px");
    let pinned = |p: &str| get(p).is_some_and(|v| zero(&v));
    let (mut top, mut right, mut bottom, mut left) = (
        pinned("top"),
        pinned("right"),
        pinned("bottom"),
        pinned("left"),
    );
    if let Some(inset) = get("inset") {
        let v: Vec<&str> = inset.split_whitespace().collect();
        let [t, r, b, l] = match v.as_slice() {
            [a] => [*a, *a, *a, *a],
            [a, b] => [*a, *b, *a, *b],
            [a, b, c] => [*a, *b, *c, *b],
            [a, b, c, d] => [*a, *b, *c, *d],
            _ => return None,
        };
        (top, right, bottom, left) = (zero(t), zero(r), zero(b), zero(l));
    }
    let thin = |p: &str| get(p).and_then(|v| length(&v)).is_some_and(|v| v <= 6.0);
    let vertical_strip = thin("width") && (left != right);
    let horizontal_strip = thin("height") && (top != bottom);
    (vertical_strip || horizontal_strip).then(|| background.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(css: &str, rule: CssRule) -> (usize, usize) {
        let r = check(css);
        (r.violations(rule), r.excluded(rule))
    }

    #[test]
    fn gradient_on_a_face_is_a_violation_but_a_meter_is_excluded() {
        let css = ".head{background:linear-gradient(180deg,#222,#111)}\n.meter .f{background:linear-gradient(0deg,#0f0,#f00)}";
        assert_eq!(count(css, CssRule::Gradient), (1, 1));
    }

    #[test]
    fn borders() {
        let css = ".a{border:1px solid #333}.b{border:0}.c{border-radius:8px}.d{border-left:3px solid red}";
        assert_eq!(count(css, CssRule::Border), (1, 0));
        assert_eq!(count(css, CssRule::OneSided), (1, 0));
    }

    #[test]
    fn tinted_backgrounds() {
        let css = ".a{background:rgba(239,90,80,.16)}.b{background:#fff}.c{background:var(--alert-tint)}.veil{background:rgba(0,0,0,.5)}";
        assert_eq!(count(css, CssRule::Tint), (2, 1));
    }

    #[test]
    fn one_sided_shadows_include_neutral_light_but_not_a_soft_outer_shadow() {
        let css = ".a{box-shadow:inset 3px 0 0 var(--c)}.b{box-shadow:inset 0 1px 0 rgba(255,255,255,.1)}.c{box-shadow:0 18px 48px rgba(0,0,0,.5)}.d{box-shadow:inset 0 0 0 1px var(--x)}.e{box-shadow:0 -1px 0 #000}.f{box-shadow:0 12px 32px rgba(0,0,0,.5),0 2px 6px rgba(0,0,0,.3)}";
        assert_eq!(count(css, CssRule::OneSided), (3, 0));
    }

    #[test]
    fn edge_strip_is_one_sided() {
        let css = ".a::before{position:absolute;left:0;top:0;bottom:0;width:3px;background:var(--c)}.b{position:absolute;inset:0;background:var(--c)}";
        assert_eq!(count(css, CssRule::OneSided), (1, 0));
    }

    #[test]
    fn keyframes_and_comments_are_ignored() {
        let css = "/* border:1px solid red */@keyframes k{from{border:1px solid red}}.a{color:red}";
        assert_eq!(check(css).total_violations(), 0);
    }

    #[test]
    fn style_block_in_html_keeps_line_numbers() {
        let html = "<html>\n<style>\n.a{\nborder:1px solid red}\n</style>";
        let r = check(html);
        assert_eq!(r.findings[0].line, 4);
    }
}
