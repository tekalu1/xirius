# LAME（MP3 の書き出し）

Xirius は MP3 の書き出し（EXP-005）に LAME 3.100 を使う。LAME のライセンスは
GNU Library General Public License 2.0 or later（LGPL-2.0-or-later、全文は `COPYING.txt`）で、
Xirius（MIT OR Apache-2.0）には組み込まない。

- LAME は cargo の依存に入れない（`cargo deny check` は LGPL を落とすまま）。
- アプリは実行時に別のファイルの動的ライブラリを読む（`libloading`、ISC）。
  - Windows: `lame_enc.dll`（LAME の `lame_*` の関数をそのまま出す。BladeEnc の `beInitStream` などではない）
  - macOS: `libmp3lame.dylib`
  - Linux: `libmp3lame.so`（無ければ `libmp3lame.so.0` と、システムに入っているもの）
- 探す順: 環境変数 `XIRIUS_LAME`（ファイルのパス。指定したときはそれだけを使う）→ 実行ファイルと同じフォルダー
  （試験は `target/<profile>/deps` から走るので、その 1 つ上も見る。macOS は `../Frameworks` も）。
- 同じ名前の別のビルドに置き換えると、次の起動からそちらを使う（LGPL の差し替えの自由）。
- 読めないときは MP3 だけを止め、WAV と FLAC は書き出せる。

## 出どころ

| | |
|---|---|
| 版 | 3.100（改変なし） |
| ソース | https://downloads.sourceforge.net/project/lame/lame/3.100/lame-3.100.tar.gz |
| SHA-256 | `ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e` |
| プロジェクト | https://lame.sourceforge.io/ |

## 作り方

ライブラリはリポジトリに入れない（バイナリで、下のスクリプトで同じものを作れる）。
どのスクリプトも、上のソースを落として SHA-256 を確かめ、ライブラリと `LAME-COPYING.txt`（LAME の
`COPYING`）・`LAME-SOURCE.txt`（出どころ）を `target/lame/<OS・arch>/` に置き、`target/debug`・
`target/release` にも写す（アプリと試験はそこで探す）。

### Windows

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-lame.ps1            # x64
powershell -ExecutionPolicy Bypass -File scripts/build-lame.ps1 -Arch arm64
```

Visual Studio 2022（C++ の build tools）があれば作れる。MSVC で `libmp3lame` と `mpglib` をそのまま
compile し、LAME が配る `include/lame.def` の関数を出す `lame_enc.dll` を link する（CRT は静的、
依存は KERNEL32.dll だけ）。`target/debug`・`target/release` に写すのは x64 だけ。

### macOS・Linux

```sh
scripts/build-lame.sh               # 作って target/debug・target/release に写す
scripts/build-lame.sh --no-install  # 作るだけ
```

C コンパイラー・make・curl・tar があれば作れる（macOS は Xcode の Command Line Tools）。
LAME の `configure` と `make` で、ライブラリだけ（コマンドの `lame` は無し）を作る。

- macOS: arm64 と x86_64 を別々に作り（macOS 11 以降）、`lipo` で 1 つの `libmp3lame.dylib` にする。
  install name は `@rpath/libmp3lame.dylib`。
- Linux: その PC の arch の `libmp3lame.so`。
- 3.100 は `include/libmp3lame.sym` に、もう無い `lame_init_old` を載せていて、Apple の linker が
  止まる。作るときに、展開した写しからその 1 行だけ消す（Homebrew も同じ。ソースは直さない）。

Linux は WSL（Ubuntu 24.04、x86_64）で作り、アプリが読む 16 の関数が出ていて、48 kHz・192 kbps の
固定ビットレートで書けることを確かめた。macOS はまだ実機で作っていない。

## 配り方

ライブラリを実行ファイルから探せる場所に置き、`LAME-COPYING.txt` と `LAME-SOURCE.txt` を一緒に入れる。
「Xirius について」の画面（LIC-015）に、ライブラリの場所・ライセンス文・ソースへのリンクを出す。

| OS | 置く場所 | 一緒に入れるもの |
|---|---|---|
| Windows | `xirius.exe` と同じフォルダー（`lame_enc.dll`） | `LAME-COPYING.txt`・`LAME-SOURCE.txt` |
| macOS | `Xirius.app/Contents/Frameworks/libmp3lame.dylib` | 同じ 2 つを `Contents/Resources/` に |
| Linux | 実行ファイルと同じフォルダー（`libmp3lame.so`） | 同じ 2 つ |

- macOS は、`.app` を署名するときにライブラリも同じ Developer ID で署名する（`codesign --options runtime`）。
  別の署名のライブラリは Hardened Runtime に止められる。利用者が差し替えたものを読むには、アプリに
  `com.apple.security.cs.disable-library-validation` が要る。
- Linux の配布（deb・Flatpak など）は、同梱せずにディストリビューションの `libmp3lame0` に頼ってもよい
  （アプリは `libmp3lame.so.0` も探す）。
- どの OS でも、利用者は同じ名前の別のビルドに置き換えられる（LGPL の差し替えの自由）。
