# ARA のホストの形（PLG-039）

ARA のホストを作る前に決めることと、その根拠の試しをまとめる。決めたのは次の 3 つ。

1. ARA の API は **C のヘッダー（`ARA_API`）を Rust から直に束縛する**。`ARA_Library` の C++ の殻は使わない。
2. ARA のプラグインは**同じプロセス**で動かす（PLG-015）。落ちても本体が落ちない囲い（PLG-058）は、**ホスト側の例外の囲い**で作る。
3. 囲いで足りないと分かったときの逃げ道は、**ARA だけの別プロセス**（本文の「別プロセスにする場合」）。いまは作らない。

## 1. 束縛の形: C のヘッダーを直に束縛する

2026-10-14 に `Celemony/ARA_SDK` の `releases/2.3.0`（Apache-2.0。`NOTICE.txt` を同梱する。LIC-004）を取って見た。

- ARA の本体は `ARA_API/ARAInterface.h`（3798 行）の **C の構造体と関数ポインター**だけ。ホストが渡すコントローラーは 5 つ（`ARAAudioAccessControllerInterface`・`ARAArchivingControllerInterface`・`ARAContentAccessControllerInterface`・`ARAModelUpdateControllerInterface`・`ARAPlaybackControllerInterface`）、プラグインから受けるのは `ARADocumentControllerInterface` など。どれも `#[repr(C)]` の構造体と `extern "C"` の関数で書ける。
- SDK の `ARA_Examples/MiniHost/MiniHost.c`（540 行）は**純 C のホスト**で、C++ 無しでホストが書けることを示している。Rust で同じ形を書く。
- `ARA_Library/Dispatch/ARAHostDispatch.{h,cpp}`（計 1474 行）は C++ のクラスで、`ARADocumentControllerInterface` などの呼び出しを包むだけ。使うと C++ の殻と C の層が要るうえ、例外と ABI の境が増える。Rust で包み直すほうが小さい。
- VST3 側の結び付け（`ARAVST3.h` の `IMainFactory`・`IPlugInEntryPoint2`）は COM の vtable なので、今の `xirius-vst3` と同じ形で手で書く。CLAP 側は `ARACLAP.h`（拡張の取得だけ）。
- 束縛は、`bindgen` を**手元で一度だけ**走らせて出た Rust のファイルを置く（ビルドで libclang を要求しない）。この PC では Swift の toolchain の `libclang.dll` があるが、CI や他の開発者に libclang を課さないため。ヘッダーは `ARAInterface.h` が C99 で、数百の型だけなので、手直しも小さい。
- 取り込む物: `ARAInterface.h`・`ARAVST3.h`・`ARACLAP.h`・`ARAAudioFileChunks.h` と `LICENSE.txt`・`NOTICE.txt`。`install_JUCE_ARA.cmake` は使わず、取り込まない（JUCE は使わない）。`3rdParty`（cpp-base64・pugixml・ICST AudioFile）は取り込まない。

## 2. 同じプロセスのままで、落ちても本体が落ちないか

ARA はホストのモデルをプラグインが任意のスレッドから読みに来るので、別プロセスに出せない（`ARA_Library`・`ARA_Examples` の汎用の IPC は SDK 側も「後の版に延期」）。PLG-015 はその前提。一方、同じプロセスのプラグインの不正なメモリのアクセスは普通ならプロセスごと落とす。そこで、次の囲いを試した。

### 試した囲い

`crates/xirius-plugins/tests/ara_containment.rs`（Windows の x64 のみ）。

- ホストがプラグインを呼ぶとき、**呼ぶ専用のスレッド**で呼び、そのスレッドに「いまプラグインの中」の目印を付ける。
- プロセスに vectored exception handler を 1 つ付ける。目印の付いたスレッドで `EXCEPTION_ACCESS_VIOLATION` か `EXCEPTION_ILLEGAL_INSTRUCTION` が起きたら、原因を記録して**そのスレッドだけ `ExitThread`**。目印の無いスレッドの例外は触らず、通常の経路に回す。
- 返らない呼び出しは、時間切れ（試験では 150 ms）で見つけて `TerminateThread`。

### 結果

| 起こすこと | 結果 |
|---|---|
| 正常な呼び出し | 戻り値が返る |
| null への書き込み（アクセス違反） | 呼び出しのスレッドだけ終わり、原因が記録され、試験が続く |
| 未定義の命令 | 同じく囲える |
| 返らない呼び出し | 時間切れで見つかり、スレッドを止められる |
| その後の呼び出し | 普通に動く |

### 囲えないこと（この形の限界）

- **ヒープを壊す書き込み**。壊れた後に、無関係の場所で落ちる。落ちた場所がプラグインの外なので、目印では見分けられない。
- **ロックを持ったままのスレッドを終わらせた**とき。ホストとプラグインが共有するロック（メモリ割り当てを含む）が解放されず、あとで固まる。
- **スタックあふれ**（`STATUS_STACK_OVERFLOW`）。ハンドラーを動かす余裕が無いことがある。
- **プラグインが自分で作ったスレッド**での違反。目印が無い。例外の**アドレスがプラグインのモジュールの範囲にあるか**で見分ける形に広げる必要がある（`GetModuleHandleExW` の `FROM_ADDRESS`）。この試験ではまだ試していない。
- **オーディオスレッド**。cpal のコールバックのスレッドを `ExitThread` で止めると音が止まる。プラグインの描画は、ホストの**ワーカースレッド**（`ENG` の並列の実行）で呼び、コールバックは結果の入ったバッファを受けるだけにする。ワーカーが落ちた間は元の音（ARA を付ける前のクリップの音）を鳴らす。試験は無い。

### 決めたこと

- 囲いは上の形で作る。足すもの: モジュールの範囲での見分け、ワーカーで呼ぶこと、落ちたら「落ちた」の状態を立てて PLG-055 の印（色だけ。文は出さない）にすること、再起動の入口（`plugin.restart`）。
- 「囲えない」ものは、囲いを**保証と言わない**。ヒープが壊れたら本体が落ちうる。PLG-058 の `done_when` のうち「落ちても本体が落ちず」は、**アクセス違反・未定義の命令・返らない呼び出し**について満たす。ヒープの破壊・ロックの取りこぼしは満たさない。これを docs と報告に書いておく。
- 実機のプラグイン（Melodyne・Gliss）で、解析中・編集中に落とす試し（PLG-038）で、囲えない落ち方が実際に出るかを確かめる。出るなら、次の別プロセスの形に進む。

### 別プロセスにする場合（逃げ道。いまは作らない）

ARA のプラグインとそのモデルのコントローラーだけを子のプロセスに出し、親は `ARAAudioAccessControllerInterface` などの**親→子の呼び出しを IPC で中継**する。

- 子の側に、ホストのモデル（オーディオソースのサンプル）を**共有メモリ**で見せる。読み出しはプラグインのスレッドから随時来るので、子の中で親のコントローラーのスタブが共有メモリを読む。
- 親の側に、プラグインのコントローラー（`ARADocumentControllerInterface` の 40 ほどの関数）のスタブが要る。
- これは SDK が延期した汎用の IPC を自作することになり、PLG-015 を変える。囲いの結果が悪かったときだけ、PLG-015 を直してから作る。

## 3. 試験の代役（ARA の相手）

SDK の `ARA_Examples/TestPlugIn` は C++ で、VST3 SDK を取り込まないと作れない。Xirius の試験用には、`crates/xirius-test-vst3` の隣に **`xirius-test-ara`**（Rust の cdylib）を置く設計にする。

- VST3 の束として `ARA Main Factory Class` の `IMainFactory` を出し、`IPlugInEntryPoint2` で `ARADocumentControllerInterface` を返す。
- 中身は小さい。オーディオソースが渡されたら、**サンプルを `ARAAudioAccessControllerInterface` から読んで RMS の列を作る解析**をして、解析の進み（`ARAModelUpdateControllerInterface` の `notifyAudioSourceAnalysisProgress`）を段階で通知し、終わったら content reader にノートを 1 つ返す。
- 試験用の環境変数で、「解析の途中でアクセス違反」「描画で返らない」を起こす。囲いの試験（PLG-058）はこれで走らせる。
- 本物の相手は Gliss（作者の ARA のプラグイン）と Melodyne で、PLG-038 の検証表で確かめる。

## 4. ARA の実装を今回は止める理由

- 段 3 の項目（LIC-004、PLG-031〜PLG-038、PLG-047、PLG-049、PLG-015、ENG-025、EDT-023、ARC-025、PLG-058）はどれも未着手で、`ARA_API` の束縛・5 つのコントローラー・VST3 の結び付け・モデルの対応・解析の待ち・保存・再生リージョンの差し替え・代役のプラグイン・編集の欄の画面までが 1 つの流れになる。ミキサーとプラグインの 1.0（01〜03）の作業の枠に入らない大きさ。
- 上の試しで、PLG-015 と PLG-058 は**両立の見込みがある**ことは分かったが、実機のプラグインでの試しが無いと、囲いが足りるかは決められない。
- そのため 04（`clip.ara.*`・Ctrl+M・クリップのメニューの項目・解析中の表示・編集の欄）は作らない。承認済みの形はリポジトリの外の調査の資料（`reports/ux-v1-mixer/`）にあり、`docs/ui-approved.md` には足さない（作っていない形を書かないため）。

## 5. 次の手順

1. LIC-004: `ARA_API` のヘッダー 4 つと `LICENSE.txt`・`NOTICE.txt` を取り込み、`cargo deny` と NOTICE の一覧を通す。
2. PLG-031・PLG-033: 束縛と VST3 の `IMainFactory` の結び付け。`xirius-test-ara` の代役で、ドキュメントを作ってプラグインに結び付けるところまで。
3. PLG-032・PLG-034・PLG-035: モデルの対応・メッセージスレッド・解析の待ち。
4. この囲い（`ara_containment.rs` の形）をワーカー呼び出しに組み込み、PLG-058 の試験を代役で走らせる。
5. PLG-038: Gliss・Melodyne で解析中・編集中に落とし、囲えない落ち方が出るかで、別プロセスの形に進むか決める。
6. EDT-023・UI: `clip.ara.attach`・`clip.ara.detach`・`clip.ara.open`・`clip.ara.status`・`plugin.restart`（ARA）と、Ctrl+M・Ctrl+Alt+M、クリップのメニュー、解析中の %、編集の欄。
