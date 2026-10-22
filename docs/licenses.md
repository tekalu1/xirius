# ライセンスの方針と同梱する一覧（LIC-007・LIC-024）

## 何を同梱するか

| ファイル | 中身 | 作り方 |
|---|---|---|
| `NOTICE` | Xirius の著作権表示と、組み込んだほかの人の物（書体・LAME・写したコード）の出どころ | 手で書く |
| `THIRD-PARTY-LICENSES.txt` | アプリに組み込んだ Rust のライブラリ（名前・版・ライセンス）と、ライセンスの文。その後に、ソースを写して一緒にビルドするほかの人のコード（`crates/*/vendor` の signalsmith-stretch・signalsmith-linear）を同じ形で（LIC-021） | `scripts/licenses.py` で作る |
| LAME の `COPYING` | LGPL の文（`xirius-session` の `mp3::LAME_LICENSE`） | LAME の版に付いてくる |
| `licenses/octicons-LICENSE.txt` | 「Xirius について」の GitHub の印（Octicons の `mark-github`、MIT、GitHub Inc.）のライセンスの文。元は `crates/xirius-ui/assets/octicons/LICENSE`（UI-303） | Octicons の版に付いてくる |

アプリは `THIRD-PARTY-LICENSES.txt` と LAME の `COPYING`（と書体の OFL の文、Octicons の MIT の文）を `include_str!` で読み込んで、「Xirius について」から開けるようにしている（`crates/xirius-ui/src/about.rs`）。
そのため、exe だけを配っても、ライブラリの一覧とライセンスの文が一緒に届く。
インストーラーや zip を作るときは、exe の隣にも `NOTICE`・`THIRD-PARTY-LICENSES.txt`・`LICENSE-MIT`・`LICENSE-APACHE` を置く。LAME の DLL を同梱するなら、その隣に LAME の `COPYING` も置く。

## 作り直す手順

依存を足したとき、または `Cargo.lock` が変わったときに作り直す。

```sh
cargo install cargo-about --locked --features cli   # 初回だけ。0.9 系
python scripts/licenses.py          # THIRD-PARTY-LICENSES.txt を書く
python scripts/licenses.py --check  # 古いと exit 1
```

- 設定は `about.toml`（許すライセンスは `deny.toml` と同じ）、形は `about.hbs`。
- 対象は配る 4 つのターゲット（Windows・macOS の x86_64 と aarch64）の、配るクレートだけ。開発用（dev-dependencies）とビルド用（build-dependencies）、ワークスペースのクレートは入らない。
- ネットワークを使うのは `cargo fetch --locked` だけ。cargo-about は `--frozen` で動かし、取ってきたクレートのライセンスのファイルを読む（ClearlyDefined などには問い合わせない）。
- 出力からは、この PC のパス（`C:\Users\…`・`.cargo/registry`）を除く。入っていると止まる。
- cargo-about はクレートしか書かないので、`vendor/` と `third_party/` の下に写したコードは、`scripts/licenses.py` の `VENDORED`（名前・版・ライセンス・ライセンスの文のファイル）から後ろに足す。
  アプリに入らないもの（`third_party/vst3sdk` は試験のプラグインを作るためだけ）は `NOT_SHIPPED` に理由と一緒に書く。
  どちらにも無いフォルダーがあると、`python scripts/licenses.py` も `--check` も止まる。ライセンスの文が書いたライセンスで始まらないときも止まる。

## 古くならないための確かめ

- `cargo test -p xirius-ui` の `the_list_of_the_rust_libraries_is_the_lock_files`: 一覧の各ライブラリが `Cargo.lock` に同じ版で入っていることを確かめる。依存の版を上げて作り直していないと落ちる。
- 足したライブラリが一覧に無いことは、この試験では分からない（ターゲットで絞るので、`Cargo.lock` の全部とは比べられない）。依存を足したら `python scripts/licenses.py --check` を流す。
- `cargo test -p xirius-ui` の `copied_code_that_is_not_a_crate_is_listed_with_its_licence_text`: `crates/*/vendor` の下のフォルダーがすべて一覧にあり、ライセンスの文が「ライセンス文」で開くものに入っていることを確かめる。
- `python scripts/test_licenses.py`: `vendor/`・`third_party/` の下で一覧に無いものと、違うライセンスの文で止まることを確かめる。

## 生成物をリポジトリに入れる理由

`THIRD-PARTY-LICENSES.txt` は生成物だが、リポジトリに入れている。

- アプリが `include_str!` で読むため、ビルドのたびに cargo-about を要求せずに済む（cargo-about が無い PC や CI でもビルドが通る）。
- 一覧の変化が差分として見え、レビューで新しいライセンスに気付ける。
- ソースの配布物（リポジトリの zip）にも、そのまま入る。


## 依存の許可・禁止

Xirius 自身は `MIT OR Apache-2.0`。依存の追加・更新では、推移的な依存と、写して組み込むコードも同じ条件で確かめる。
MIT・Apache-2.0・BSD・ISC・Zlib などの許可型を使う。許す SPDX の識別子の完全な一覧は `deny.toml` の `[licenses].allow`、生成する一覧の条件は `about.toml` にある。
MPL-2.0 はファイル単位の条件を守って使える。変更した MPL のファイルはその条件で公開する。二重ライセンスは許す側を選ぶ。

GPL・AGPL・LGPL・SSPL と、許可リストに無い独自ライセンスのコードをアプリに組み込まない。次のものは使わない。

| 対象 | 理由・扱い |
|---|---|
| JUCE（`juce`・`juce-sys`） | AGPL / 商用の条件をアプリに持ち込まない |
| Tracktion Engine（`tracktion`・`tracktion-engine`） | GPL / 商用の条件を持ち込まない |
| `vst3-sys` | MIT 化より前の GPL の束縛。`vst3` を使う |
| nih-plug の VST3 出力（`nih_plug`・`nih_plug_derive`） | `vst3-sys` を引くため、クレートも禁止 |
| Rubber Band（`rubberband`・`rubberband-sys`） | GPL の時間伸縮。signalsmith-stretch を使う |
| yabridge・Carla・Ardour など GPL の DAW / ホストのコード | コードを写さない |
| VST2 SDK | 新規の契約を結べないため取り込まない |
| ARA SDK の `install_JUCE_ARA.cmake` | JUCE を取得するため使わない |

例外は、実行時に読み込む別ファイルの LAME（LGPL）だけ（LIC-006・LIC-015）。Cargo の依存やアプリへの静的な組み込みにはせず、DLL / dylib / so を同梱する。
ライセンスの文、同じ版のソースの入手先、利用者が同名のライブラリに差し替えられることを「Xirius について」と配布物で案内する。
ASIO SDK はリポジトリに入れず、既定で無効にする。配布する場合は配布者が契約と表示の条件を満たす（LIC-011）。

`cargo deny check` がクレートの許可リストと名前の禁止リストを検査する。Cargo の外のコードと LAME の同梱条件は、この文書と `scripts/licenses.py --check`・配布物の検査で確かめる。
