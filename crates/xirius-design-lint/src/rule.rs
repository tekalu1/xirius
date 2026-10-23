//! 規則の一覧。

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rule {
    OpsCoverage,
    /// 生の色（`rgb(0x…)`・`hsla(`・`rgba(`・`#rrggbb` の文字列）。
    RawColor,
    /// 生の角丸（数値を直に渡す・`rounded_md` のような既定値）。
    RawRadius,
    /// 書体名の文字列。
    RawFont,
    /// 線（`.border_*`）。
    Border,
    /// グラデーション。
    Gradient,
    /// 角丸の物の 1 辺だけの色。
    OneSidedEdge,
    /// 細い線（太さが小さい固定値の面）。構造線（`StructureLine`）を通した物だけ通る。
    Line,
    /// 画面・コマンドの文言の硬い言い回し（「要ります」「格子」など）。
    Wording,
    /// 画面の文字の中の、操作を教える文（「クリックで追加します」「〜してください」など）。
    Guidance,
    TextOpacity,
    TranslucentTextSurface,
    ColorFamily,
    ExceptionFamily,
    /// 有彩色の塗りの面の上の文字・アイコンが `ink_on_fill`（その面の対の色）でない（UI-309）。
    FillInk,
    /// 一覧のメニューを PickList でなくドロップダウンの `Menu` で作る（UI-338）。
    ListMenu,
    /// 例外の印に理由が無い。
    AllowWithoutReason,
    /// 例外の印の規則名が知らない物。
    UnknownRule,
    /// 例外の印が、何も止めていない。
    UnusedAllow,
    /// ソースが読めない。
    ParseError,
}

impl Rule {
    /// 例外の印（`design-lint: allow(…)`）で外せる規則。
    pub const CONTENT: [Rule; 15] = [
        Rule::RawColor,
        Rule::RawRadius,
        Rule::RawFont,
        Rule::Border,
        Rule::Gradient,
        Rule::OneSidedEdge,
        Rule::Line,
        Rule::Wording,
        Rule::Guidance,
        Rule::TextOpacity,
        Rule::TranslucentTextSurface,
        Rule::ColorFamily,
        Rule::ExceptionFamily,
        Rule::FillInk,
        Rule::ListMenu,
    ];

    pub const ALL: [Rule; 20] = [
        Rule::OpsCoverage,
        Rule::RawColor,
        Rule::RawRadius,
        Rule::RawFont,
        Rule::Border,
        Rule::Gradient,
        Rule::OneSidedEdge,
        Rule::Line,
        Rule::Wording,
        Rule::Guidance,
        Rule::TextOpacity,
        Rule::TranslucentTextSurface,
        Rule::ColorFamily,
        Rule::ExceptionFamily,
        Rule::FillInk,
        Rule::ListMenu,
        Rule::AllowWithoutReason,
        Rule::UnknownRule,
        Rule::UnusedAllow,
        Rule::ParseError,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Rule::OpsCoverage => "ops-coverage",
            Rule::RawColor => "raw-color",
            Rule::RawRadius => "raw-radius",
            Rule::RawFont => "raw-font",
            Rule::Border => "border",
            Rule::Gradient => "gradient",
            Rule::OneSidedEdge => "one-sided-edge",
            Rule::Line => "line",
            Rule::Wording => "wording",
            Rule::Guidance => "guidance",
            Rule::TextOpacity => "text-opacity",
            Rule::TranslucentTextSurface => "translucent-text-surface",
            Rule::ColorFamily => "color-family",
            Rule::ExceptionFamily => "exception-family",
            Rule::FillInk => "fill-ink",
            Rule::ListMenu => "list-menu",
            Rule::AllowWithoutReason => "allow-without-reason",
            Rule::UnknownRule => "unknown-rule",
            Rule::UnusedAllow => "unused-allow",
            Rule::ParseError => "parse-error",
        }
    }

    pub const fn summary(self) -> &'static str {
        match self {
            Rule::OpsCoverage => "画面の操作はコマンド表に登録する",
            Rule::RawColor => "生の色は xirius-design のトークンを通す",
            Rule::RawRadius => "角丸は xirius_design::Radius から取る",
            Rule::RawFont => "書体は xirius_design::Font から取る",
            Rule::Border => "線で区切らない。面の差と余白で分ける",
            Rule::Gradient => "グラデーションは値を表す物（メーター）だけ",
            Rule::OneSidedEdge => "角丸の物の 1 辺だけに色・光・影を付けない（中立の色でも）",
            Rule::Line => "細い線を引かない（例外は xirius_design::StructureLine だけ）",
            Rule::Wording => "文言は製品の自然な日本語と、DAW で通じる用語で書く",
            Rule::Guidance => {
                "画面の文字に、操作を教える文を置かない（アイコン・配置・状態で伝える）"
            }
            Rule::TextOpacity => "文字の色に不透明度を掛けない",
            Rule::TranslucentTextSurface => "薄い面の上に文字を置かない",
            Rule::ColorFamily => "surface / fill / ink / line の用途を守る",
            Rule::ExceptionFamily => "meter / track / wave / alert の用途を守る",
            Rule::FillInk => "有彩色の塗りの面の上は ink_on_fill（mute・solo の札はその対の色）",
            Rule::ListMenu => {
                "一覧は PickList で作る（ドロップダウンの Menu は操作のメニューだけ）"
            }
            Rule::AllowWithoutReason => "例外の印には理由を書く",
            Rule::UnknownRule => "例外の印の規則名が違う",
            Rule::UnusedAllow => "何も止めていない例外の印は消す",
            Rule::ParseError => "Rust として読めない",
        }
    }

    pub fn from_name(name: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|r| r.name() == name)
    }

    /// 例外の印で外してよい規則か。
    pub fn is_allowable(self) -> bool {
        Rule::CONTENT.contains(&self)
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
