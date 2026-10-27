# UI の I/O と完了の境界（PRJ-031）

通常の GPUI 窓は `Studio::bind_window_io` で worker を使う。CLI、オフラインの Session、
撮影・計測用の同期アダプターとは入口を分ける。起動時の encoder・インストール先・OS の
motion 設定は、GPUI executor と窓を作る前に worker で取得する。起動前の join は、この
bootstrap だけに限る。窓を作った後は完了を poll する。

## 操作と所有者

| 操作 | I/O・待ちを実行する場所 | UI が扱う物 |
|---|---|---|
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
