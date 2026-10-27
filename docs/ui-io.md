# UI の I/O と完了の境界（PRJ-031）

通常の GPUI 窓は `Studio::bind_window_io` で worker を使う。CLI、オフラインの Session、
撮影・計測用の同期アダプターとは入口を分ける。起動時の encoder・インストール先・OS の
motion 設定は、GPUI executor と窓を作る前に worker で取得する。起動前の join は、この
bootstrap だけに限る。窓を作った後は完了を poll する。

## 操作と所有者

| 操作 | I/O・待ちを実行する場所 | UI が扱う物 |
|---|---|---|
| 接続 MCP の Session コマンド全体 | 実 Session の worker。open/save は専用の所有者プロトコルを先に使う | 受付と完了。名前の分類から漏れた拡張も UI では実行しない |
| 場所・お気に入り・非表示の Session コマンド | Session worker。先に受付済みの Browser 保存を待ち、同じファイルの read/modify/write を直列化する | Browser の差分保存を維持する。待つのは worker だけ |
| パレットの問い合わせ | 同じ Session worker | 以前と同じ結果フォーム。メニューの Session dry-run は引数の検査だけでディスクを読まない |
| UI マクロ | `run_parsed` と `run_command` から同じ継続へ入り、tick/idle が所有者の完了後に次へ進む | 表示用 Session は編集・保存・履歴移動を拒否する。最後と失敗・中止時の group 終端は実 Session に対して行う |
| UI の異常時ログ | application FIFO の `log`。stderr の file/pipe も worker | メッセージの受付。終了時は受付済み出力の完了を確認する |
| パネル寸法の読込・保存 | `ancillary_io` の FIFO | snapshot と完了。読込中の drag・reset は古い snapshot より優先する |
| 版の一覧・詳細 | 同じ FIFO。保存先・lane・モデルごとのキャッシュ | 一覧、現在選んだ版の詳細。保存途中の自動版は Session のメモリーから補う |
| cleanup のメニュー・確認一覧 | FIFO の `read_view` | 曲と素材参照所有者が一致する一覧。実行時にも worker が使用中の素材を確認する |
| グルーヴ MIDI の取り込み | FIFO の `read_groove` | 曲の所有者が一致した結果だけ適用する |
| 初回編集時の下書き | 次の idle・drag 終了後に `Saving` の Session worker | 同じ callback 内の後続編集と押している音を保持する。下書きは保存済みとは扱わない |
| 保存後に不要になった下書き | application FIFO | 削除要求の受付。通常の曲を下書きとして削除しない |
| history・version・file・cleanup の実行 | `io_command` から既存の Session worker | 完了後の選択復元、一覧更新、MCP の返答 |
| About・更新確認・クラッシュ記録・license | application FIFO。ダウンロードは既存の worker | 既存の画面に完成した値だけ反映する。検証が済むまで終了・インストールを予約しない |
| ファイル・URL を OS で開く | application FIFO または `os` の desktop worker | 受付。OS プロセスの生成失敗は worker が記録する |
| cleanup の繰り越し・終了通知 | application FIFO | 繰り越し一覧は UI の受領後に該当行だけ acknowledgement する |
| 最後の窓の終了処理 | application FIFO | executor は停止せず、installer 再検証・起動、cleanup の待ち・通知書込の完了を確認する |
| MCP の起動 | `start_window` の所有者 worker | readiness の完了だけ受け取り、サーバーを Screen に保持する |
| OS の motion 設定の再取得 | `MotionWatch` の worker | 前回の値と完了 |
| 外部変更の監視 | `other_change::window` の専用 worker | 監視登録・metadata・project の read・watcher の破棄を UI に戻さない |

recent.json、設定・プラグイン一覧・preset・ブラウザー・波形・素材・書き出し・装置・MIDI・
SDK の所有者は PRJ-029/030 と BRW-019 の worker を引き続き使う。共有 mutex の中は
メモリー上の snapshot・要求・受領の操作に限り、I/O の前に解放する。履歴 writer の
完了ゲートも pending 件数の mutex を保持しない。ゲートが閉じている間は件数を pending
に保ち、UI の照会が返るようにする。

## 結果の受け入れと終了

FIFO の read ticket を捨てると、まだ実行していない read は省く。実行中の I/O を強制停止
せず、破棄した受取先へ結果を適用しない。受付済み write は detached として残し、最後の
窓が閉じても executor が pending を確認する。I/O は返信用 mutex を保持しない。失敗と
panic は結果またはログに残し、後続の FIFO 処理を継続する。

外部変更は read 要求の保存済み fingerprint・モデル・要求番号を照合する。「読み込む」
より前の要求で届いた結果をその確認の承認には使わない。新しい保存や編集は古い read
より優先する。core の `ExternalProject` は読んだ Store と fingerprint を保持し、受け入れ
時にも照合する。履歴 step と manifest は同じ writer の FIFO に送る。再生中の Session と
SDK を read worker へ移さず、既存の通常編集の音の更新を使う。Undo は一段で行える。

recent の終了期限、曲本体の保存・未保存確認、録音の所有権と片付け、RT の制約は維持する。
停止した driver・SDK・OS 呼び出しの強制終了は行わない。停止中の所有者が資源を保持しても、
窓の callback はその資源の join・recv・I/O を待たない。

## 検出と試験

`ui-blocking-io` は UI ソースの標準ファイル API（import の alias を含む）、ファイルに
到達し得るメソッド、プロセス生成、標準出力マクロ、同期受信・待ち・join・mutex を検出する。既存の同期
アダプター・worker 内の helper・メモリーだけの同名操作には、該当する呼出し行に理由を
書く。通常画面のモジュール全体を除外しない。撮影台本そのものと `cfg(test)` の本体は
別の対象とする。新しい呼出しには別の監査が必要になる。

これは Rust 全体の呼出しグラフを証明する仕組みではない。別クレートの計測された helper
への到達も検出するため、操作別のゲート試験は `io_test::forbid_ui_io` で実際の UI callback を囲む。
`loading-probe` では本文・overlay の実描画にも同じ検査を掛ける。メモリー上の受領通知
（Peaks の peek 後、recent の空受信、writer 完了）をファイル I/O とは数えない。
writer の flush 待ちは禁止対象のままにする。

`ancillary_io` の単体試験は実 read を閉じたまま入力・tick・idle・geometry の Ack を確認し、
同じ未完了要求を繰り返しても read が一件であること、古い read の省略、write の順序と
panic 後の継続を確認する。外部変更と writer は採用・保存・完了の順序も確認する。
合否はゲートの前後関係で決め、壁時計の待ち時間は停止を検出する watchdog だけに使う。

`live_edit_window` は既存の素材・SDK・録音・設定・書き出し・装置ゲートに、寸法・版・
cleanup・グルーヴ・通知・外部監視・クラッシュ記録・更新確認・ログのゲートを加える。
私有の fixture、無音の出力、画面外の自分の窓だけを使い、PrintWindow と対象 HWND への
入力で描画・移動・resize・最小化・復帰、および再生と editor の継続を検査する。

## レビュー後の入口・継続の再監査

直接の標準 API に加え、Session の動的実行、`run_and_persist`、UI コマンドの呼び出し、
場所・更新・cleanup の既知 I/O helper を lint 対象に広げた。import の別名も展開する。
`assert!` 等の式引数、`write!` / `writeln!` の出力先、未知のマクロの入れ子と呼び出し形の
トークンも検査する。worker の名前だけでは closure を除外せず、既知の所有者と import を確認する。
各呼び出しの根拠はその行の注釈に残す。通常画面全体を除外する許可は加えていない。

`cargo run -p xirius-design-lint -- --audit-ui-io` は、許可済みのものも含む呼び出し位置を出す。
`--ui-io-inventory` の安定した集計は [ui-io-audit.tsv](ui-io-audit.tsv) に記録する。
`ui_io_inventory` の CI 試験はこの集計と実ソースを照合するため、許可済みの行に呼び出しを
足した場合も黙って増えない。行の移動だけでは更新を求めない。更新時は呼び出しの所有者を
再確認し、この表と注釈を直してから集計を取り直す。Rust 全体の型付き呼び出しグラフを
証明するものではないため、実入口のゲートと共通ヘルパーの実行拒否も併用する。

| 入口からの経路 | 再監査で確認・修正した境界 | 固定した競合・継続 |
|---|---|---|
| `ai_job` → Session / SessionExt → places | 全 MCP コマンドの実行は所有者へ渡す。通常 UI の places 7 コマンドも分類する | read/write を止めた実 MCP 入口。Browser 保存との両順序、同じ項目の変更と別項目の差分統合 |
| `ai.batch` / `ai.decide` → 内側の command | `execution_targets` は token を消費せず、別名・内側の対象を解決する。権限・単回 token・rollback は Core の実行を通す | yes/no・再使用、失敗バッチのモデル復元、録音中と保持テイク中の装置変更、SDK 回収失敗時の token 保持 |
| 複合コマンド内の保存・置き換え | 保存の専用プロトコルを一般分類より先に使う。内側の保存も SDK 回収の対象にする。UI バッチ内の曲置き換え・別名保存は実行前に拒否し、個別の要求窓口を使う | 読み取りは保存フラグがあっても SDK 回収を待たない。独立 CLI の同期実行は維持 |
| マクロ → UI dispatch → Session worker → 継続 | 実 Session が戻るまで後続と group 終端を保留する。別の曲へ替わった場合は明示して停止する。保存の fork も group を引き継ぐ | 読み取り前後の複数編集、一段 Undo、次の編集との分離、失敗時の末尾停止。実窓の `macro_version_list` でも SDK/editor と後続編集を確認する |
| パレットの Read 問い合わせ・メニューの dry-run | Read や dry-run という属性だけでは I/O なしと扱わない。結果は worker からフォームへ戻す | `file.list` の実列挙を止めたパレット入口。`Song::check` の Session 分岐はメモリー上の引数検査だけ |
| MIDI 学習の置き換え → unset / set | UI で unset した直後に Session を渡す継続も除去し、2 操作を所有者で行う | 同じ複合実行の権限・完了管理を使う。通常 UI から未監査の Session handler を呼ぶ場合は `Song::ensure_inline` が拒否する |
| 更新 startup / 手動確認 → ready installer | release の受領を共通化し、不一致の ready と Ready phase を解除する | A の installer と B の回答を同時に受領しても、B の既存 Download ボタンに戻る |
| cleanup read → キャッシュ受領 → 確認窓 | 一覧の受領と表示予約を分ける。入力・コマンド・別 popup の意図で予約を無効にする | read 保持中の Esc と別 popup。遅着した一覧が後の画面を置き換えない |
| export.cancel の別プロセス向け分岐 | 自分のジョブの中止通知と、ディスク上の cancel 要求を分ける。窓から後者へ直接入る場合は拒否する | 通常の command は worker、窓の scan 中止は所有中ジョブのメモリー通知だけ |

## 利用者の操作と遅着結果の再監査

PRJ-030 の準備結果と同じく、worker は結果を返し、採否は UI が所有する要求と世代で決める。
曲が一致するだけでは採用しない。操作を変える結果には、その操作の適用権も必要になる。

| 入口・結果 | 優先順位と採用条件 | 確認 |
|---|---|---|
| 接続 MCP → Session 引渡し | `human_interaction_active` を受付の最初に判定し、操作中は明示的な busy を返す。Read・保存・曲の置換も同じ境界を通す | ノート移動・長さ変更のプレビュー保持、MouseUp の確定と Undo、終了後の再試行、PC 鍵盤の押下保持 |
| 初回下書き・外部変更の自動採用 | 同じ操作中判定を共有する。pointer、実 drag、pan、ruler、鍵盤、文字入力、OS ファイル選択中は開始・採用を待つ | 元の pointer 保護に加え、PC 鍵盤とファイル選択の保留を確認。外部変更は既存の request 番号・model・保存 fingerprint の照合も維持 |
| グルーヴ read → 自動選択 | `Intent::accepts` に曲・世代・要求時の editor と設定・外部グルーヴの照合を集約。後の設定操作・editor 選択・再ドロップは古い適用権を失わせる | Groove(false)、Clip、Preset、External、同じ値へ戻す操作、editor の再選択。古い成功も失敗も新しい設定・通知を上書きしない |
| OS ファイル・フォルダー・設定の選択 | 3つの native prompt 継続を `PickerIntent` の曲と世代で照合する。古い返答は新しい予約を解除しない。選択中の MCP と自動採用も保留する | 古い返答と新しい予約の競合、曲の置換後の返答拒否、終了後の予約解除を OS の窓なしで確認 |
| Browser 一覧・波形・設定再読込 | 既存の保存先・モデル・要求世代を照合し、設定は読み取り後の差分を統合する | BRW-019 と既存の停止ゲート・差分統合試験を維持。戻ったデータだけで選択を戻さない |
| drawer / side 寸法 | 要求時の値と、要求後に操作した印の両方を照合する | 後から変更し同じ寸法へ戻した場合も古い read を採らない既存試験を維持 |
| MIDI・音の準備・更新・cleanup | MIDI と準備は所有者の世代、更新は channel と installer の候補、cleanup は曲・model・popup intent を照合する | 既存の採否判定を再確認。更新の手動再確認は startup も含む `asking()` が直列化し、後の要求が古い回答に追い越されない |

lint は標準 API の関数値参照（別名や callback への受け渡しも含む）と、Read / Write・同期受信・
lock / join の標準 UFCS 表記も検出する。集計はこの**名前ベースの走査で見つかった直接境界**であり、
任意の trait 実装・別クレートの helper の内部変更や全入口の安全性を証明しない。
helper の本体だけが変わる場合も件数照合では検出できないため、所有者を制限する実行境界と、
実入口の停止ゲート・採否の回帰試験を併用する。
