# プラグインのホスト

## 実装の順序（PLG-030）

ホストは VST3 → CLAP → ARA の順に作る。前の段が `docs/plan.toml` で done になるまで、次の段の項目に手を付けない。

| 段 | 形式 | 項目 | 前の段 |
|---|---|---|---|
| 1 | VST3 | PLG-001・PLG-027・LIC-003・PLG-051 | なし |
| 2 | CLAP | PLG-028・PLG-006（あとで PLG-026） | 段 1 |
| 3 | ARA | PLG-039（形を決める）→ LIC-004 → PLG-031〜PLG-038・PLG-047・PLG-049・PLG-015・ENG-025・EDT-023・ARC-025 | 段 2 |

- VST3 は市販のプラグインの大半が出している形式で、ARA も VST3 のコンパニオン API（`IMainFactory`）から結び付けるので最初に作る。
- CLAP は VST3 と同じ形（読み込み・パラメーター・状態・別プロセス）で足す。ARA の CLAP 版（ARACLAP）もこの上に乗る。
- ARA は SDK の取り込み（LIC-004）とホストの形（PLG-039）を決めてから作る。それまでは、ARA のプラグインを別プロセスに出さないことだけを先に守る（PLG-025。段 3 の前に入れる）。

## 曲を開くときの開き方（ENG-064）

- 曲を開くと、`xirius_session::starting::Starting` が曲のプラグインを少しずつ開く。最初の 1 歩で、別プロセスのプラグインを開く指示をまとめて送り、返事を待たない。続く 1 歩ごとに、同じプロセスのプラグインを 1 つ開く（窓は 1 フレームに 1 つ）。別プロセスの返事は届いた順に数える。
- 共有のホスト（`Separate`）は 1 つのプロセスなので、その中では 1 つずつ開く。同時に開くのはプラグインごとのプロセス（`Dedicated`・`PerPlugin`）だけ。
- すべて開いたら、開いたものをそのまま使って再生の道を組み、装置を開く（設定の排他モードのとおり）。開けなかったプラグインは「…。代わりのインストゥルメントで再生します」と言い、曲は開く。
- 各プラグインの開いた時間は `Builder::timings`（`Timings::opened`）とログの `plugin_opened owner= name= ms= process=host|here` に残る。全体は `song_prepared ms= units= ok=`。
- `SessionHost::start`（CLI・MCP・`xirius play`）も同じ道を通り、途中で止まらずに最後まで開く。`Starting::finish`・`SessionHost::sync`・`restart_plugin(s)` も開く返事を待つ形のままで、`commands.rs`・`ext.rs`・`record.rs`・`xirius play` はこれを使う（ENG-065 で変えない）。

### 窓が開く返事を待たない開き方（ENG-065）

窓（`xirius-ui`）は、開く返事を待たない形を使う。

- 「後で読む」の後は `Starting::finish_late` で、別プロセスのプラグインを開く返事を待たずに再生の道を組む。まだ開いていない音源は無音（`RemotePlugin` は接続が有効になるまで素通し）のままで、開いたものから `PluginServer` が順に取り付ける。同じプロセスのプラグインは、これまでどおり要ったときに 1 フレームに 1 つ開く（VST3 の規則）。
- 差し替え（`SessionHost::sync_later`）は、`step_swap` を 1 フレームに 1 回呼んで進める。1 回目で別プロセスのプラグインの読み込みをまとめて始め、続く回で同じプロセスのプラグインを 1 つずつ読み、全部読めて準備中が無くなったら、鳴っている組を新しい組に差し替える。それまで鳴っている組は止めないので、ほかのトラックは鳴り続ける。差し替え中は `swap_pending()` が真で、光った鍵のスピナーが回る。
- 差し替えの間、同じプラグインの音源はホストの中に 2 つある。`PluginLink` は、鳴っている方（`slot`。窓と報告はこちらのもの）と、新しい組が開いた方（`next`）の 2 つの席を持つ（ENG-066）。曲から与える値（`set_param`・`set_state`・オートメーション）は両方に送り、鳴っている方の窓で動かした値と、鳴っている方が報告した状態は新しい方へも送る。差し替えを取り消して新しい方を閉じたら席を空けるだけで、鳴っている方は与えた値のまま残る。差し替えた時（`swap_all`）に新しい方を `slot` に上げ（`PluginServer::promote`）、開いている窓をそちらへ移す。古い組を離して古い方を閉じるのは、音声のスレッドが新しい組を取った後なので、それを待たない（待つと、その間の窓の操作や鳴っている方への命令が、もう鳴らない古い方に届く）。取り消しや差し替えの時に値を送り直さない。
- 音源が言ったこと（開けなかった訳・開いた返事・ミックスに足す遅れと組んだ時のプラグインの遅れ・開くのにかかった時間）も、`LinkInner` の 2 つの席（鳴っている方の `heard`、新しい方の `coming`）に分けて持つ（ENG-067）。書き手の席は、その音源のスロットを `slot`・`next` と比べてサーバーの錠の中で決める（`State::seat_of`。ホストに届く前に落ちた開き方はスロット 0 で、ほかが鳴っていれば新しい方）。窓・状態の報告・遅れの組み直し（`error`・`opened`・`status`・`take_latency_change`）は `heard` を見るので、保留中に新しい方が開けなくても、鳴っている方の表示も音も変わらない。組んだ時の警告と開いた時間（`opening_error`・`opening`・`open_took`）は新しい方を見る。差し替えた時に `coming` が `heard` になり、取り消したら消える。開けなかった新しい方で差し替えたときの警告は、すぐ組んだときと同じ「読み込めません（バイパスして再生します）」。
- ホストが送る状態（`FromHost::State`）は、鳴っている方のものだけを曲への編集（`take_edits`・`states_seen`）として受ける。差し替えで窓を移すと古い方の窓を閉じ、ホストは窓を閉じた時の決まりでその状態を読み直すが、それはもう鳴っていない方のものなので受けない（受けると、新しい方で動かした値が古い方の状態で上書きされる）。
- 窓で値を動かすと、動き（`FromHost::Gesture`）が先に届き、状態はそれを鳴らした後（2 ブロックの後。鳴っていなければ 300 ms 後）に届く。画面は毎回 `collect_plugin_states` で取り込むので、両方が同じ回に入るとは限らない。曲が名前で持つ値は、状態の無い回の動きをプラグインごとに取っておき（`PluginLoader::moves_until`）、状態が届いた回に合わせる（ENG-071。前は同じ回に入ったときだけ合わせていて、分かれると曲の値が前のまま残った）。
- 遅れが変わった時の組み直し（`SwapKind::Latency`）も差し替えで進める。差し替えを待つ間に遅れが変われば、その差し替えに「遅れも待つ」印（`PendingSwap::latency`）を付ける。待つ形の `sync`（窓の編集を曲へ取り込んだ直後に呼ばれる）や、曲が鳴っている組と同じに戻った時は、曲のための差し替えを捨てるが、印のあるものは遅れのための差し替えとして残し、値の変わった曲から組む（ENG-068。捨てると、プラグインは新しい遅れで鳴り、ミックスは古い遅れのまま補う）。
- 窓で値を動かすと、その音源をもう一度開く時の値（サーバーの `SlotRec::open`）にも書く。差し替えや再起動で今の値から開き直す時に、最初に開いた時の値へ戻さないため（ENG-069。曲が別の値を持っていたプラグインで、状態は新しく、値だけが古いまま開き、窓で動かした値が消えていた）。
- 分け方を変えて別のホストのプロセスへ移る音源は、新しいホストで新しい `PluginLink` になる。古い音源の窓が開いていれば、新しいリンクにも窓が開いている印と題を付け（`PluginServer::open_editor_later`）、新しい組が入った時（`promote`）に新しいホストへ窓を開かせる。待つ間は古い窓が開いたままで、状態も開いたままと言うので、画面はそれを閉じた窓と取らない（ENG-070。それまでは新しいリンクの窓が閉じたままで、画面が窓を消していた）。同じプロセスの中と別のプロセスどうしの移り方の話で、同じプロセスと別のプロセスの間は次の項。
- 同じプロセス（「なし」）と別のプロセスの間を移る音源は、新しい組が開いてから入るまで（`PluginLoader::open` から `promote_opened`・`drop_ready` まで）、鳴っている古い音源を `PluginLoader::leaving` に取っておく（ENG-071）。窓・状態の報告（`reports`・`plugin_statuses` の遅れと `editor_open`）・値の動き（`set_live_param`・窓で動かした値の取り込み）・`local` は、鳴っている方から読む。`link`・`server_of`・`placement_of` は新しく組んだ方のままで、鳴っている方のリンクは `playing_link`。差し替えを捨てたら（`drop_ready`）古い方を元の席へ戻す。
  - 別のプロセスからこのプロセスへ移る音源は、新しい組が入った時にホストの窓がまだ開いていれば、ホストの窓を閉じ、古いホストが古い音源を手放してから（下の項）印を付け（`window_moved_in`）、画面がこのプロセスで開き直す。手放すまでは `window_moving_in` が真で、画面は窓を消さずに待つ。待つ間に閉じた窓は、画面がそれに気づく前に組が入っても開き直さない。
  - 人が古いホストの窓を閉じたのが組の入る直前だと、ホストの「閉じた」（`FromHost::EditorClosed`）は組が入った後に届く。ホストは音源を外す時（`ToHost::Close`）に、閉じられていた窓の `EditorClosed` を先に送ってから `FromHost::Closed` を返す（プロトコル 6）。サーバーは外した音源のリンクを `Closed` まで `State::closing` に取っておき、その間に届いた `EditorClosed` も数える（`PluginLink::windows_shut`）。`promote_opened` は組が入った時の数を覚え、`settle_following`（`SessionHost::poll` から）が、古いホストが手放した（`PluginServer::has_let_go`）時までに数が増えていれば、新しいホストの窓を閉じ、このプロセスでは開かない。手放す返事が来ないホストは 2 秒（`HAND_OVER`）で待つのをやめる。それまでは時間だけで待っていて、遅れて届いた「閉じた」を外した後のスロットとして捨て、閉じた窓が新しい音源で開き直していた。
  - 画面（`mixer/editors.rs`）は、ホストに開かせた窓を、ホストが一度「開いている」と言うまでは最長 3 秒開いているものと取る。一度言った後は、ホストの言うことだけで決める（閉じたと言えば 3 秒を待たずに消す）。
  - 書き出し（`export::render_with`）では、組んだミックスがすぐ鳴るので、組むたびに `leaving` を空ける（`PluginLoader::built_offline`）。同じプロセスの音源は、組み直すたびに古い音源を `leaving` に入れる。ステムのように 1 つの Builder で何度も組むと、最初の回の音源が書き出しの終わりまで残り、`local` もそれを返していた。
  - このプロセスから別のプロセスへ移る音源は、画面のこのプロセスの窓が新しい組の入った時に古くなり（`local` の世代が変わる）、画面が新しいホストで開き直す。
  - 別のホストどうしでも、待つ間に古い窓を閉じたら（画面からでも、ホストでの人の操作でも）、新しいリンクの「窓が開いている」印を消し（`promote_opened`）、新しいホストで開かない。
- `plugin.isolation.set` と、プラグインのプロセスが落ちた後の開き直し（`restart_plugin(s)_later`）も同じ道を通る。開き直しは、生きている古いホストが終わるのを待つ分（最長 5 秒）だけ待つ。
- 別プロセスのプラグインを開いた時に出た警告は、返事が来てから `take_late_warnings` で届く（`プラグインを読み込めません` の警告は今までと同じ文言）。
- 装置を失った後の 2 秒ごとの組み直し（`Transport::reopen` → `ensure`）も `start_late` で組むので、別プロセスのプラグインの返事は待たない。同じプロセスのプラグインは組み直しの 1 回で全部開く（これは変えていない。装置が戻るまでの再試行で、窓は鍵を待たせる場面ではない）。

## VST3 SDK の版（LIC-003）

VST3 SDK は 3.8.0 から MIT。3.7.x 以前（GPLv3 か Steinberg の個別契約）の写しはリポジトリに入れない。

- `third_party/vst3sdk` に submodule として固定する。版は `v3.8.0_build_66`（`9fad9770f2ae8542ab1a548a68c1ad1ac690abe0`）。中身はリポジトリに入れず、gitlink だけを持つ。
- ビルドは submodule を読まない。ホストは `vst3` crate 0.3（MIT OR Apache-2.0）のバインディングを使い、これは SDK 3.8.0 の pluginterfaces から生成されている（`vst3::Steinberg::Vst::SDKVersionString` は `VST 3.8.0`）。
- submodule は、SDK の試験道具（validator・hostchecker）をビルドするときだけ取り出す（下の「試験道具」）。
- `crates/xirius-vst3/tests/sdk_pin.rs` が次を確かめる。
  - バインディングの版が 3.8 以上であること。
  - `third_party/vst3sdk` が mode 160000 で、固定した commit を指していること。
  - SDK のヘッダー（`pluginterfaces/`・`ivstaudioprocessor.h`・`ipluginbase.h`・`vstversion.h`）の写しが追跡されていないこと。

SDK を上げるときは、submodule の commit・`sdk_pin.rs` の `PINNED`・この節を同時に直す。

## CLAP（PLG-028）

CLAP のホストは clack（`clack-host`・`clack-extensions` 0.2、MIT OR Apache-2.0）で書く。VST3 と同じ形にそろえ、エンジン・曲・別プロセスのホストは 2 つの形式を同じに扱う。

| 層 | VST3 | CLAP |
|---|---|---|
| 読み込み・パラメーター・状態・処理 | `xirius-vst3` | `xirius-clap` |
| エンジンの処理（同じプロセス） | `Vst3Processor` | `xirius_engine::clap_plugin::ClapProcessor` |
| 一覧（スキャン） | `.vst3` | `.clap`（`CLAP_PATH` と標準のフォルダー） |
| 別プロセスのホスト | `xirius-plugin-host` | 同じ exe・同じメッセージ |

- 曲は CLAP のプラグインを `format = "clap"`、`uid` = CLAP の id（`com.vendor.name`）で持つ。状態は VST3 と同じ文字列で、プラグインのバイト列を component の側に入れ、controller は空にする。
- 別プロセスのメッセージはクラスを 16 バイトで渡す。CLAP では id を `xirius_clap::class_key`（FNV-1a 128 ビット）にして渡し、ホストは `.clap` の中からその鍵のプラグインを選ぶ。メッセージの形は変えない。
- 別プロセスでは、VST3 と同じく落ちても乾いた音が残り、再起動で状態が戻る。エディターでの編集（`gesture` と値）は VST3 と同じ道で曲に入り、1 つの取り消しの単位になる。
- CLAP のエディター（`gui` 拡張）はまだ開かない。開こうとすると「CLAP のエディターはまだ開けません」を返す。同じプロセスの CLAP は、曲の値を変えると新しいミックスで反映する。
- CLAP の restart の要求は無視する。値・状態の変化（`state.mark_dirty`・`params.rescan`）は VST3 の kParamValuesChanged と同じに扱う。

試験:

- `crates/xirius-clap/tests/` は `xirius-test-clap`（自作の CLAP、MIT OR Apache-2.0）で読み込み・パラメーター・状態・音・ノートを確かめる。
- `crates/xirius-engine/tests/sandbox_clap.rs` は別プロセスのホストで鳴らし、編集の後の状態がホストの再起動で戻ることを確かめる。
- `crates/xirius-plugins/tests/scan_clap.rs` は `.clap` が一覧に入り、キャッシュから読み直せることを確かめる。
- `crates/xirius-session/tests/clap.rs` は曲の CLAP を同じプロセスと別プロセスで鳴らし、編集が曲の状態に入り、その曲を開き直すと同じ音になることを確かめる。

## ARA のプラグインの置き場所（PLG-025）

ARA のプラグインは、ホストの ARA の物（曲の音の読み出し）を同じプロセスで受け取る。別プロセスに出すと届かないので、ARA のホスト（段 3）を作るまでも、ARA のプラグインは必ずこのプロセスで動かす。ARA の SDK はまだ取り込まない（LIC-004）。見分けは VST3 のクラスの一覧だけで足りる。

- スキャンは、VST3 の束が分類 `ARA Main Factory Class` のクラスを持つか、音のクラスの副分類に `OnlyARA` があれば、その一覧の行に `ara: true` を付ける（`xirius_plugins::ara`）。一覧のキャッシュは版 2 になる。アプリは起動の後に版の古いキャッシュを見つけると、無いときと同じくスキャンし直す（`CacheFile::is_current`）。
- CLAP の ARA（ARACLAP）はまだ見分けない（`ara: false`）。
- 曲が `shared`・`dedicated` を指していても、プレイヤーは ARA のプラグインをこのプロセスで動かし、「（名前）: ARA のプラグインのため、このプロセスで実行します（（指定）の指定）」の警告を出す。曲の指定は書き換えない。
- プラグインのメニューの「動かす場所」は「アプリ内」だけを選べ、点いているのも「アプリ内」になる。落ちたときの「専用で動かす」も出さない。

試験:

- `crates/xirius-plugins/src/ara.rs` はクラスの一覧から見分ける規則を確かめる。
- `crates/xirius-session/tests/ara.rs` は、ARA の印の付いた一覧で `shared`・`dedicated` の曲を組むと別プロセスのホストを立てずに警告を 1 つ出し、印が無ければ曲の指定どおりに出すことを確かめる。
- `crates/xirius-ui/src/health.rs` の `an_ara_plugin_runs_here_only` は、メニューに別プロセスの選択肢が出ないことを確かめる。
- 2026-10-13 にこの PC の標準のフォルダー（491 束・741 個）をスキャンし、ARA と見分けたのは Melodyne・Synthesizer V Studio ARA Plugin・Synthesizer V Studio 2 ARA Plugin・VOCALOID Bridge（`OnlyARA`）・Gliss の 5 個だった。

## 試験道具（PLG-042）

規格の側の試験道具で、自作の試験プラグイン（`xirius-test-vst3`・`xirius-test-clap`）が規格どおりに振る舞うことと、ホストが規格のサンプルを正しく動かすことを確かめる。外から取る物は、リポジトリの外の空のフォルダー（以下 `EXT`）に置く。音は出さず、窓も開かない。

### VST3 validator（SDK 3.8.0）

プラグインを試す道具。自作の試験プラグインを通し、ホストの試験の前提が規格に合っていることを確かめる。

```sh
git clone --depth 1 --branch v3.8.0_build_66 --recurse-submodules --shallow-submodules \
  https://github.com/steinbergmedia/vst3sdk.git EXT/vst3sdk
cmake -S EXT/vst3sdk -B EXT/build -G "Visual Studio 17 2022" -A x64 \
  -DSMTG_ENABLE_VSTGUI_SUPPORT=OFF -DSMTG_ADD_VST3_PLUGINS_SAMPLES=OFF \
  -DSMTG_CREATE_PLUGIN_LINK=OFF -DSMTG_ADD_VST3_HOSTING_SAMPLES=ON
cmake --build EXT/build --target validator --config Release
cargo build -p xirius-test-vst3
# validator は .vst3 の束（フォルダーの形）を読む
mkdir -p EXT/bundles/XiriusTest.vst3/Contents/x86_64-win
cp target/debug/xirius_test_vst3.dll EXT/bundles/XiriusTest.vst3/Contents/x86_64-win/XiriusTest.vst3
EXT/build/bin/Release/validator.exe EXT/bundles/XiriusTest.vst3
EXT/build/bin/Release/validator.exe -e EXT/bundles/XiriusTest.vst3   # 拡張の試験も
```

### SDK のサンプルプラグイン（host-checker ほか）

`host-checker` はホストの側を採点するプラグイン。`again`・`adelay`・`note-expression-synth` は振る舞いがソースから分かるので、ホストの音の結果を確かめられる。`crates/xirius-vst3/tests/sdk_samples.rs` がこれらを窓も装置も無しで動かす。環境変数が無いときは飛ばす。

```sh
cmake -S EXT/vst3sdk -B EXT/build-samples -G "Visual Studio 17 2022" -A x64 \
  -DSMTG_ENABLE_VSTGUI_SUPPORT=ON -DSMTG_ADD_VST3_PLUGINS_SAMPLES=ON \
  -DSMTG_CREATE_PLUGIN_LINK=OFF -DSMTG_ADD_VST3_HOSTING_SAMPLES=OFF -DSMTG_RUN_VST_VALIDATOR=OFF
cmake --build EXT/build-samples --config Release --target host-checker again adelay note-expression-synth
XIRIUS_VST3_SDK_SAMPLES=EXT/build-samples/VST3/Release cargo test -p xirius-vst3 --test sdk_samples
```

### clap-validator

CLAP のプラグインを試す道具（free-audio/clap-validator、MIT）。CLAP にはホストを採点する道具が無いので、自作の試験 CLAP が規格どおりであることを確かめ、ホストの試験の前提にする。

- 取得元: <https://github.com/free-audio/clap-validator> の Windows 版の配布物 `clap-validator-0.4.1-127-g152b982-windows.zip`。
- SHA-256: zip は `d935c3af0a45c3911ea2e900f4aa5d6709dac82bb485f0c4ce28648ab2cd0c10`、中の `clap-validator.exe` は `825439ad3ff98770a24656013eeb11591407687b97058e1db9549c4f7d53e38a`。

```sh
cargo build -p xirius-test-clap
cp target/debug/xirius_test_clap.dll EXT/bundles/xirius_test_clap.clap
EXT/clap-validator/clap-validator.exe validate --json EXT/bundles/xirius_test_clap.clap
```

### ARA TestPlugIn

ARA の SDK を取り込むまで動かさない（PLG-025・PLG-030）。ARA のホストを作るときに、この節へ手順と結果を足す。
ホストの形（C のヘッダーを直に束縛する）と、同じプロセスでの落ちの囲いの試しは `docs/ara-host.md`（PLG-039）。

### 結果（2026-10-13、Windows 11・x64）

| 道具 | 対象 | 結果 |
|---|---|---|
| VST3 validator | `xirius-test-vst3` | 47 件すべて通過 |
| VST3 validator `-e` | `xirius-test-vst3` | 537 件すべて通過 |
| SDK のサンプル（`sdk_samples.rs`） | host-checker・again・adelay・note-expression-synth | 4 件すべて通過。host-checker の採点は 2 から 8 に上がる |
| clap-validator | `xirius-test-clap` | 44 件のうち 35 件通過、9 件は対象外（Unix だけの試験・preset-discovery・64 ビットの音・ポートの構成を持たないため）、失敗 0 |
| clap-validator | Vital（参考。ホストではなくプラグインの側の結果） | 25 件通過、11 件対象外、6 件失敗、2 件は 45 秒で止まった |

- validator に通すため、`xirius-test-vst3` のバスに名前（`Stereo`）を付け、バッファの無い process（パラメーターの flush）を受けるようにした。
- clap-validator の `state-reproducibility-*` が、状態の読み込みの後に `params.rescan` を出していない試験 CLAP を見つけた。試験 CLAP は読み込みの後に rescan を出すように直し、ホスト（`xirius_clap::Plugin::set_state`）は自分で入れた状態の後の rescan と dirty を編集として扱わない。
