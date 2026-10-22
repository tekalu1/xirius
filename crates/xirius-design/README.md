# xirius-design

Xirius のデザイントークン（色・寸法・動き・書体）。GPUI に依存しない、値だけの crate。
決めたデザインの規則を、使う側が破れない形にしてある（規則の一覧は `docs/design-rules.md`）。

## 使い方

```rust
use xirius_design::{Surface, Theme, TrackColor, Pair, surface, ink};

let theme = Theme::Dark;

// 面と文字。許された組だけが作れる。
let text = Surface::S1.text().resolve(theme);          // 本文の色と面の色
let pair = Pair::<surface::Fill, ink::OnFill>::new();  // 型で縛る版
let _ = pair.resolve(theme);

// トラックの色は、中身（ノート・波形）とアイコンに。クリップの面は Surface::Clip。
let note = TrackColor::new(3).content(theme);
```

色は `Color`（`to_rgba_u32()` が GPUI の `rgba()` にそのまま渡せる `0xRRGGBBAA`）。

## 規則を型で守る仕組み

- 文字 × 面は `Pair` / `Readable`。一覧（`pair.rs` の `allowed_pairs!`）に無い組は作れない。
  一覧は両テーマで 4.5:1 以上を `tests/contrast.rs` が確かめる。薄い文字の段は無い。
- グラデーションの型は `meter` にしか無い。線の太さの型は無い。
- パレットの欄は `pub(crate)`。生の色は、`Surface`・`Ink`・`Accent`・`TrackColor` 越しにしか取れない。
- 書体は `Font` の 2 つだけ。

## 値の元

色・余白・角丸・文字の大きさ・行間・太さ・書体名・動き・部品の寸法の値は `tokens.json` だけにある。
`src/generated.rs` は、そこから作った Rust（手で直さない）。値を変えるときは `tokens.json` を直し、

```
cargo run -p xirius-design-lint -- tokens --write
```

で作り直す。元と合わなければ、`cargo run -p xirius-design-lint` と `cargo test -p xirius-design-lint` が落ちる。
将来の Web・QML の出力は、同じ `tokens.json` から作る。

## 検査

```
cargo test -p xirius-design
```

## 書体

同梱する書体は Inter と Noto Sans JP（OFL）。この crate は表だけを持ち、ファイルとライセンスの文はリポジトリの `assets/fonts/` にある（`xirius-ui` の exe に組み込む）。`fonts/README.md` を見る。

## 出どころ

面・文字・塗りの値は Pleiad のトークンを元にした。
コントラストのために変えた値は `src/palette.rs` の頭に書いてある。
