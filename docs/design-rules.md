# デザインの規則と、それを守らせる仕組み

規則（UI-103〜111・UI-050・UI-090 ほか）を、**破れない API・テスト・lint・CI** の 4 層で守らせる。
規則の正本は `docs/plan.toml`。ここは、規則が**どこで止まるか**の表と、使い方。

## 規則と、止める場所

| 規則（plan の項目） | 破れない API（`xirius-design`） | テスト（`cargo test`） | lint（`xirius-design-lint`） |
|---|---|---|---|
| 文字と面の組は 4.5:1 以上（UI-106・UI-056） | 許す組だけを型にした `Pair<S, I>`（`ReadableOn`）。許されない組は `compile_fail` | 全部の許す組 × 2 テーマの比を計算（最小 4.60:1） | — |
| 設定のオフのトグルの溝は周囲の面と 3:1 以上、つまみも溝と 3:1 以上（UI-317） | `SwitchFace::off` が溝とつまみの色を組で返す | 両テーマ・ホバー前後で設定の中立の面との比を計算。画面の共通トグルが同じ組を使うことも検査 | — |
| 薄い文字を置かない（UI-107） | `Ink` に弱い段が無い | `api_surface`: ソースに `weak` が無い | — |
| 薄い色の面の上に文字を置かない（UI-108） | `*_tint` のトークンが無い・`Surface` に文字を置ける組だけ | 同上 | — |
| 線を引かない（UI-109・UI-050） | 線（border）の API も、上辺の光の API も無い。区切りは面の段の明るさの差と、浮く面の外側の影（`FloatShadow`）だけ | `api_surface`: `border` が無い。`surface_steps`: 隣り合う面の段の差 ΔL* ≥ 1.25（暗い・明るい両方） | `border`: `.border_*` |
| グラデーションを使わない（UI-105・UI-090） | グラデーションは `meter` モジュールの `LevelMeter` だけ | `api_surface`: `gradient` の語は `meter.rs` だけ | `gradient` |
| 細い線を引かない。例外はフォルダーの範囲の構造線（MIX-038）と、時間のグリッドの線（UI-339）だけ | 線の型は `StructureLine`（太さ 2px・両端丸・溝の中・hover/選択でアクセント）と `GridLine`（小節 2px・ほか 1px、段ごとの色としきい値）の 2 つだけ | `contrast`: 構造線が中立の面の上で 3:1 以上。グリッドの 4 段がどの面の上でも 小節 > 拍 > 分割 > 細かい。`structure`・`grid` の単体テスト | `line`: 太さが 3px 以下の固定値の面、グリッドの色を `GridLine` を通さずに塗る `Pen` の面 |
| 角丸の物の 1 辺だけに色・光・影を付けない（UI-103・UI-007） | 1 辺の色・光・影を作る API が無い | — | `one-sided-edge` |
| 色・書体・角丸はトークンだけ（UI-110） | 値は `Surface`・`Ink`・`Accent`・`TrackColor`・`Radius`・`Font` から取る | — | `raw-color`・`raw-radius`・`raw-font` |
| 書体は Inter と Noto Sans JP だけ（UI-111・UI-005） | `Font` は 2 値だけ | `api_surface` | `raw-font` |
| トラックの色と中立の面のコントラスト 3:1（UI-008） | `TrackColor` | 16 色 × 2 テーマ（最小 3.39:1） | — |
| 有彩色の塗りの面の上は ink_on_fill（mute・solo の札はその対の色）（UI-309） | `Fill`・`FillHover`・`Danger`・`DangerHover` の面に許す文字は `OnFill` だけ。画面の `Colors` は `primary`・`primary_hover` を面と文字の組（`Pair<Fill, OnFill>` から作る）で持ち、`Pen::transition_pair` で面と文字を一緒に切り替える | `draw::tests`: 入ったトグルは 2 テーマとも面が fill・文字が on_fill（4.5:1 以上）。`color_usage`: 今の `xirius-ui` は 0 件・もとのトグルは落ちる | `fill-ink` |
| 一覧系は検索付きの共通部品（PickList）で作る（UI-320・UI-338） | 一覧を作る API は PickList だけ（`Popup` と設定の `OpenMenu` は `pick: Option<PickList>` を持ち、一覧の腕はすべて `pick_list::cells` を通る）。ドロップダウンの `Menu` は「その他」の操作のメニューだけ | 一覧を開く試験で検索の欄・↑↓ Enter・Esc の 2 段・当たり無しの表示を数える（`pick_list::tests`、設定の一覧は `settings::tests`） | `list-menu` |

## lint の使い方

## 画面・CLI・MCP の用語（ARC-027）

| 避ける語 | 製品で使う語 |
|---|---|
| 格子 | グリッド |
| 吸着 | スナップ |
| 寄せる | クオンタイズ・スナップ |
| 素通し | バイパス |
| 強さ | ベロシティ |
| 版・版の・版を | バージョン・バージョンの・バージョンを |
| 録音待ち・録音待機 | 録音準備 |
| 入り切り | オン・オフ |
| 調べ直す | 再スキャン |
| 立て直す | 再起動 |
| 進み具合 | 進行状況 |
| 名乗る | 通知 |
| 試しに実行 | ドライラン |
| 落ちた・落ちている・落とした | クラッシュした・クラッシュしている・クラッシュさせた |
| 要ります・が要る | 必要です・が必要です |
| のどれか | のいずれか |
| を作った | を作成しました |

画面には名称・値・状態を置き、操作の説明・案内の文は置かない。
CLI・MCP の説明は「〜します」、結果は「〜しました」、エラーは「〜できません」「〜が必要です」で書く。

## lint の実行

```
cargo run -p xirius-design-lint                 # crates/ の Rust を検査（違反があれば終了コード 1）
cargo run -p xirius-design-lint -- --list-rules # 規則の一覧
cargo run -p xirius-design-lint -- css <html>   # モックの CSS を数える（直さない。--deny で落とす）
```

出力は `file:line:rule: message`。設定は根の `design-lint.toml`。

- `design_crates`: 規則の持ち主（トークンの値を書く場所）。中は検査しない。
- `[[exempt]]`: 一時的に外す crate。**理由が要る**。外した crate は実行のたびに一覧に出る（黙って通さない）。
  乗り換えたら項目を消す。無い crate の項目は「消してよい」と注意が出る。
- `tests/` は既定で見ない（`include_tests = true` で見る）。`#[cfg(test)]`・`#[test]` の中も見ない。

### 例外の書き方

```rust
// design-lint: allow(raw-color) 外部の仕様が決めた色（ここだけ）
let c = rgb(0x336699);

let d = div().border_1(); // design-lint: allow(border) 同じ行にも書ける
```

- 印は、**その行**か、コメントだけの行なら**次のコード行**に効く（メソッドの連なりは、先頭の行に置けば全体に効く）。
- ファイル全体は `// design-lint: allow-file(<規則>) <理由>`。
- `,` で規則を並べられる: `allow(raw-color, raw-radius) <理由>`。
- **理由が無い印は効かない**（`allow-without-reason`）。規則名が違えば `unknown-rule`、何も止めていない印は `unused-allow`。
- 読めない Rust は `parse-error`。

### 規則の中身（要点）

- `raw-color`: `rgb(…)`・`hsla(…)`・`Hsla {…}`・`Color::hex(…)`・`black()` など、`"#rrggbb"` の文字列、`"rgba(…)"` の文字列。
- `raw-radius`: `.rounded(<数>)`・`.rounded_md()` などの既定値・`.corner_radius(<数>)`。`rounded_none()` は通す。
- `raw-font`: 書体名の文字列（Inter・Noto Sans JP・Helvetica・system-ui ほか 26 語）・`.font_family("…")`。
- `border`: `.border`・`.border_*`。
- `gradient`: 「gradient」を含む名前（`ValueGradient`・`GradientStop`・`gradient` は `xirius-design` の API なので通す）。
- `line`: `bg` を持つ面で、`.w(…)`・`.h(…)` の固定値が 3px 以下の物（区切りの線）。太さ・長さの引数に `StructureLine`・`GridLine` が出る呼び出しは数えない
  （`.w(px(StructureLine::THICKNESS))` は通る）。端に貼り付いた帯は `one-sided-edge` が止める。再生位置の針のように区切りでない物は、理由を付けて allow する。
  `Pen` の `fill`・`round`・`shape`・`floating` で、グリッドの色（`line_grid_*`・`GridLine::color`・画面の `c.grid[…]`）を、矩形に `GridLine` の出てこない所へ塗る物も違反。
  `bg` にグリッドの色を塗る物は、`GridLine` を通さなければ `color-family`。
- `one-sided-edge`: 角丸の連なりの `.border_t/b/l/r_*`、端に貼り付いた細い色の帯（`absolute` ＋ `left_0` などの 1 辺 ＋ 6px 以下の幅・高さ ＋ `bg`）、
  ぼかしも広がりも無く縦か横の一方にずらした影（`BoxShadow { offset: point(0, 1), blur_radius: 0, … }`）。
  **色が中立（白・黒・面の色）でも違反**。ぼかしのある全周の外側の影（浮く面の影）は通す。
  `Pen` で描く所では、トラックの色（`track_ink`・`color_of`・`c.tracks[…]`、それを入れた変数）を、幅か高さの一方だけが 6px 以下の `Rect::new` に塗る `fill`・`round`・`shape` も違反（AUT-014: トラックの色は線と点にだけ使う）。点（両方が小さい）は通す。
- `fill-ink`: `Pen` で描く所で、不透明な有彩色の塗りの面（`fill`・`fill_hover`・`danger`・`primary`・`primary_hover`・`arm_on`・`monitor_on`、`Surface::Fill` など）を塗った矩形の上に、
  `text`・`digits`・`icon` や、ink の色で塗った印（フェーダーのつまみの溝）を置くのに、色が `on_fill`（`ink_on_fill`）でも、その面の対（`primary`・`mute_on`・`solo_on` などの `.1`・`label`）でもない物。
  面と文字は `let (face, ink) = …` の組で選ぶ（`if on { c.fill } else { … }` と `if on { c.on_fill } else { … }` を別々に選ぶと、分岐の対応を lint は追えない）。
  塗りの輪の内側に中立の面を重ねた物（`Rect::new(r.x + 1.5, …)`）は、その上の文字を地の色にしてよい。薄く塗った面（`opacity`）は対象外（`translucent-text-surface` が見る）。
  `gpui` の `div` の木（`.bg(…).text_color(…)`）は見ない。
- `list-menu`: `xirius-ui` の画面のファイルで、ドロップダウンの `Menu::rich`・`Menu::new` を呼ぶ物（`dropdown.rs`・`pick_list.rs` と試験は除く）。選ぶ一覧（設定の一覧も）は PickList で作る。
  「その他」のような操作のメニューは、理由を付けて allow する。
- `wording`: 文字列のリテラルに、硬い言い回し（「要ります」「が要る」「格子」「吸着」「寄せる」「版の」「試しに実行」「素通し」「立て直」「進み具合」ほか 19 語）がある物。
  画面・コマンドの説明・エラーの文は、製品の自然な日本語（説明は「〜します」、結果は「〜しました」、エラーは「〜できません」「〜が必要です」）と、DAW で通じる用語（グリッド・スナップ・クオンタイズ・バイパス・バージョン）で書く。語の一覧は `scan.rs` の `STIFF_WORDS`。
- `guidance`: `xirius-ui` の画面のファイル（`crates/xirius-ui/src/`。試験のファイルは除く）の文字列に、操作を教える言い回し（「ください」「クリックで」「ダブルクリック」「右クリック」「ドラッグで」「持ち替え」「押すと」「すると」「ここに並び」「ここに表示」）がある物。
  ユーザーの決まりは「画面の文言は極力置かず、説明・案内は置かない（アイコン・配置・状態で伝える）」。空のレーンや空の一覧は、開いた状態と選択の印だけで伝え、「〜がありません。〜します」とは書かない。
  置く必要がある物（エラーの理由・取り返しのつかない操作の確認の文）は、理由を付けて allow する。CLI・MCP のエラーは対象外（呼び出す側に直し方を教えてよい）。語の一覧は `scan.rs` の `GUIDE_PHRASES`。

## 例外: 構造線（MIX-038）

「線で区切らない」の例外は、ただ 1 つ。フォルダーの範囲を、入れ子の段ごとに示す縦の線（メールの引用の線のような物）。
区切りの線ではなく、「この行はどのフォルダーの中か」を示す印として、`xirius_design::StructureLine` が持つ。

- 太さ 2px（`StructureLine::THICKNESS`）。両端は丸い（`CAP`）。行から離し、字下げの溝の真ん中（`gutter_x(深さ)`）。
  塊の角からは `END_INSET` 内側で止める。
- 色は通常は中立。フォルダーに hover しているときと選択しているときだけアクセント（`LineState`）。
  どの状態も、中立の面の上で 3:1 以上（`tests/contrast.rs`）。
- 畳んだフォルダーでは描かない（行が無いので範囲も無い）。
- lint: 細い固定値の面は `line` の違反。`StructureLine` の定数を通した物だけが通る
  （見本は `crates/xirius-design-lint/tests/fixtures/{clean/structure_line.rs,violations/line.rs}`）。
  ほかの線（区切りの線・枠・下線）は、これまでどおり `border`・`line` で止まる。

## 例外: 時間のグリッドの線（UI-339）

構造線に次ぐ 2 つ目の例外。アレンジとピアノロールの、小節・拍・分割・細かい線。区切りではなく時間の目盛りとして、
`xirius_design::GridLine` が段（`GridTier`）・太さ・色・出し始めのしきい値を 1 か所に持つ。使う側は変えられない。

- 段は 4 つ。小節（`Bar`）・拍（`Beat`。6/8・9/8・12/8 は付点 4 分）・拍の分割（`Sub`。1/2、複合拍子か 3 連のグリッドでは 1/3）・細かい線（`Fine`。分割より細かいスナップの値とフレーム、秒の線）。
- 太さは小節だけ 2px（`GridLine::BAR_THICKNESS`）、ほかは 1px（`THICKNESS`）。置く位置は `GridLine::left`（2px の線は時刻の位置を真ん中にする）。
- 色は文字の色を薄く重ねた `line_grid_bar`・`line_grid_beat`・`line_grid_sub`・`line_grid_fine`。どの面（地・hover・選択・黒鍵の行）の上でも、
  両方のテーマで 小節 > 拍 > 分割 > 細かい の比になる（`tests/contrast.rs`）。
- 小節線と拍線（拍が `MIN_BEAT_PX` 以上離れたとき）は間引かない。分割と細かい線は `MIN_FINE_PX` 以上離れたときだけ出し、出始めは `GridLine::fade` で薄く入れる。
- lint: `div` の細い固定値の面は `line` の違反で、太さ・長さの引数に `GridLine` の字がある物は通る。グリッドの色の面は、幅を `GridLine` から取った物
  （`Pen` は `Rect::new` の 3 つ目、`div` は `w` が `GridLine::width`・`THICKNESS`・`BAR_THICKNESS`）だけが通り、ほかは `Pen` なら `line`、`div` の `bg` なら `color-family`
  （見本は `crates/xirius-design-lint/tests/fixtures/{clean,violations}/ui_grid_line.rs`）。
  `Pen` の面の細い線（グリッドの色でない物）は、まだ lint では止まらない（文字のカーソル・再生位置・メーターの印など 50 か所余りが同じ形で、例外か面かを 1 つずつ決める。plan.toml の UI-342）。
  ピアノロールのほかのパートの範囲の端は、小節の `GridLine` で引く。
  ほかの線（区切りの線・枠・下線）は、これまでどおり `border`・`line` で止まる。

## 面の区切り（UI-007）

面の上辺の光は使わない（OPEN-24 で「光なし」に決めた。plan.toml の UI-007）。面どうしの区切りは次の 2 つだけ。

- **面の段の明るさの差**: `Surface` の隣り合う段（明るさの順）の CIE L* の差が 1.25 以上（`crates/xirius-design/tests/surface_steps.rs`）。
  暗い・明るいの両方で確かめる。下限の理由はテストの先頭の注。段を足す・色を変えるときは、このテストが落ちない値にする。
- **浮く面の外側の影**: `FloatShadow`。ぼかして全周に広げる。片側にだけ出る影・光・縁は、中立の色でも `one-sided-edge` の違反。

## モックの CSS の数

`cargo run -p xirius-design-lint -- css temporary/research/2026-10-11/mockups/daw-main.html [--verbose]`

グラデーション・線・薄い色の面・1 辺の色を、違反と除外（メーターの帯・フォーカスの輪・透明の枠・幕）に分けて数える。中立の光も 1 辺の色の違反に入る。
モックは直さない。実装に移すときの、残りの見積りに使う。完全な CSS の解析ではない（style 属性・JS で付ける値は数えていない）。

## CI

`.github/workflows/rust.yml`:

- Windows・macOS: `cargo fmt --check`・`cargo clippy --workspace --all-targets -- -D warnings`・`cargo test --workspace`・`cargo run -p xirius-design-lint`。
- `cargo-deny`（`deny.toml`）: ライセンスの許可の一覧は `docs/licenses.md` に合わせる。依存の条件と例外は同じ文書にまとめる。
