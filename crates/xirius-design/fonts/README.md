# fonts

Xirius が使う書体は 2 つだけ（`src/font.rs` の `FONT_FILES` が、置くファイルの表）。
どちらも SIL Open Font License 1.1。

**実体は、リポジトリの `assets/fonts/` に置いてある**（UI-002）。この crate は表だけを持つ。

| 書体 | 取得元 | 置くファイル | 大きさ |
|---|---|---|---|
| Inter 4.1 | https://github.com/rsms/inter/releases （`extras/ttf/`） | `Inter-Regular.ttf`・`Inter-Medium.ttf`・`Inter-SemiBold.ttf` | 各 約 0.41 MB |
| Noto Sans JP 2.004 | Google Fonts（`ofl/notosansjp`、元は https://github.com/notofonts/noto-cjk ）の可変フォント | `NotoSansJP-Regular.ttf`・`NotoSansJP-Medium.ttf`・`NotoSansJP-SemiBold.ttf` | 各 約 2.25 MB |

## Noto Sans JP の作り方

可変フォント（9.6 MB）から、使う文字だけ残して太さごとの固定の書体にした。

- 残す文字: ASCII・Latin-1・一般の句読点・矢印・数学記号・図形・CJK の記号とかな・全角形と半角カナ・
  JIS X 0208 の全部（漢字 6355 字を含む）。計 7358 グリフ。
- 残す機能: `kern`・`palt`・`pwid`・`tnum`・`locl`・`ccmp`・`liga`・`calt`。ヒントは捨てた。
- 太さ 400・500・600 を `fonttools` の `varLib.instancer` で取り出した（family 名は `Noto Sans JP` のまま）。
- 範囲の外の字（JIS X 0208 に無い漢字など）は、OS の書体に落ちる。

書体の名前の表（`name`）の著作権の行に、Adobe の予約名 `Source` が出る。family 名は `Noto Sans JP` で、
予約名は使っていない。ライセンス本文は書体ごとに同じフォルダーに置く（`OFL-Inter.txt`・`OFL-NotoSansJP.txt`）。

## 更新の手順

1. 取得元から元のファイルを取る。
2. Noto Sans JP は上の条件で切り出す。
3. `SHA256SUMS` を作り直す（`sha256sum` の書式。`xirius-ui` の試験が表と実ファイルの一致を見る）。
4. 配布物（インストーラー）に、ライセンス本文を同梱する。

アプリ側は `xirius_ui::fonts::load` で、`FONT_FILES` の表のとおりに GPUI に登録する。
