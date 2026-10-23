# コマンド一覧

この文書は `xirius --commands-doc docs/commands.md` でコマンド表から生成します。
検査は `xirius --commands-doc --check docs/commands.md`。

CLI は `xirius --song <曲のフォルダー> <コマンド> --json`。MCP のツール名は `.` を `_` に置き換えます。
結果は `ok`、`command`、`message`、`data`、`changes`、`dry_run`。失敗は `ok: false` と型付きの `error`。編集結果の `data.state_hash` は変更前後の状態を示します。
MCP の編集には受領証が付き、完了はイベントでも届きます。大きい結果は要約と `ops.get-result` のページで取得します。

## `ai.activity`

AI が編集中のもの

AI が編集中のものと、AI が編集してユーザーが未確認のものを表示します（UI のハイライトと枠の表示に使用）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `ai.batch`

まとめて実行

複数のコマンドを 1 つの操作としてまとめて実行します（途中で失敗した場合はすべて取り消します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `commands` | 必須 | `{"items":{"type":"object"},"type":"array"}` | [{command, args}] の配列 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `title` | — | `{"type":"string"}` | 履歴に表示するタイトル |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `ai.decide`

確認に回答

AI の操作の確認に回答します（トークンは 1 回限り・有効期限 120 秒。回答が無ければ実行しません）

事前条件: ユーザーのみ。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `answer` | 必須 | `{"enum":["yes","no"],"type":"string"}` | 実行を許可するか |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `token` | 必須 | `{"type":"string"}` | 確認トークン |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `ai.decision.get`

承認の結果

承認待ちの操作の結果を取得します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `token` | 必須 | `{"type":"string"}` | 承認トークン |

## `ai.pending`

承認待ちの確認

ユーザーの承認待ちの確認を一覧表示します（削除・上書き保存）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `ai.permission.get`

AI の権限

AI の権限を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `ai.permission.set`

AI の権限を設定

AI の権限を設定します（off・read・edit・save）。AI からの変更はユーザーの承認を待ちます

事前条件: AI からの変更はユーザーの承認が必要。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `level` | 必須 | `{"enum":["off","read","edit","save"],"type":"string"}` | 権限 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `ai.request.accept`

AI の作業コピーを取り込む

AI の作業コピーを曲に取り込みます（1 つの操作になり、取り消せます）

事前条件: ユーザーのみ。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `on_conflict` | — | `{"enum":["reject","replace"],"type":"string"}` | 作業コピーの作成後に曲が変更されていた場合の扱い（reject で中止、replace で置き換え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `ai.request.begin`

AI の依頼を開始

AI の依頼を開始します。終了までの操作を履歴の 1 つの操作にまとめます（draft では作業コピーで進め、ユーザーが取り込むまで曲は変わりません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `draft` | — | `{"type":"boolean"}` | 作業コピーで実行する |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `title` | 必須 | `{"type":"string"}` | 依頼のタイトル（履歴に表示） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `ai.request.end`

AI の依頼を終了

AI の依頼を終了します（draft の作業コピーはユーザーの確認待ちになります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `ai.request.revert`

AI の依頼を破棄

AI の依頼を破棄します（作業コピーを破棄するか、直接の編集を依頼の開始時点まで戻します）

事前条件: ユーザーのみ。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `ai.request.status`

AI の依頼の状態

進行中の依頼と、作業コピーを取り込んだ場合の変更内容を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `app.quit`

終了

窓を閉じて終了します

事前条件: ユーザーのみ。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

キー: global: Mod+Q / Mod+W / Mod+F4 (`app.quit`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `app.update.check`

更新を確認

GitHub のリリースから新しいバージョンがあるかを確認します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `audio.devices`

オーディオデバイスの一覧

オーディオデバイスの一覧と、それぞれの入力と出力の数を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `inputs` | — | `{"type":"boolean"}` | 入力のあるデバイスだけを表示する |

## `audio.get`

オーディオ設定を表示

オーディオ・MIDI の設定

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `audio.input.detect`

入力を検出

オーディオ・MIDI の設定

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `async` | — | `{"type":"boolean"}` | 検出の完了を待たずに戻る |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `audio.input.select`

入力を選択

オーディオ・MIDI の設定

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `channel` | 必須 | `{"maximum":32.0,"minimum":1.0,"type":"integer"}` | 入力（1 から） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `audio.output.set`

出力チャンネルを設定

オーディオ・MIDI の設定

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `left` | 必須 | `{"maximum":32.0,"minimum":1.0,"type":"integer"}` | 左のチャンネル（1 から） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `right` | 必須 | `{"maximum":32.0,"minimum":1.0,"type":"integer"}` | 右のチャンネル（1 から） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `audio.set`

オーディオ設定を変更

オーディオ・MIDI の設定

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `key` | 必須 | `{"type":"string"}` | 設定の名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `value` | 必須 | `{"type":"string"}` | 値 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `audio.test-tone`

テスト音

テスト音を再生します（左、右の順に約 −18 dBFS のサイン波）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `stop` | — | `{"type":"boolean"}` | 再生中のテスト音を止める |

## `auto.clear`

オートメーションのポイントを削除

レーンのポイントをまとめて消します（--range loop でループの中だけ）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `range` | — | `{"enum":["loop"],"type":"string"}` | 範囲（省略時はすべて） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.draw`

オートメーションに形を描く

オートメーションレーンの範囲に、形（直線・放物線・矩形波・三角波・ノコギリ波・サイン波・フリーハンド）を描きます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `amp` | — | `{"type":"number"}` | 波の振幅（矩形波・三角波・ノコギリ波・サイン波。省略時は範囲の 1/4） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end-value` | — | `{"type":"number"}` | 終了位置の値（省略時は開始位置と同じ） |
| `from` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `period` | — | `{"maximum":1000000000.0,"minimum":1.0,"type":"integer"}` | 波の 1 周期の長さ（tick。省略時は 1 拍） |
| `phase` | — | `{"type":"integer"}` | 波の位相のずれ（tick。省略時は 0） |
| `points` | — | `{"items":{"type":"object"},"type":"array"}` | フリーハンドの軌跡 [{tick, value}] の配列 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `shape` | 必須 | `{"enum":["free","line","parabola","square","triangle","saw","sine"],"type":"string"}` | 形 |
| `step` | — | `{"maximum":1000000000.0,"minimum":1.0,"type":"integer"}` | ポイントの間隔（tick）。フリーハンドはこの間隔で読み直し、省略すると間引く |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<ID>・param:<ID>:<名前>） |
| `to` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `value` | — | `{"type":"number"}` | 開始位置の値（フリーハンドでは不要） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.lane.add`

オートメーションレーンを追加

オートメーションレーンを追加します（非表示のレーンは再表示します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.lane.hide`

オートメーションレーンを隠す

レーンを非表示にします（ポイントは残り、再生にも反映されます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.lane.next`

パラメーターを切り替え

パラメーターを切り替え

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: automation: H (`auto.lane.next`)、arrange: H (`auto.lane.next`)、automation: Shift+H (`auto.lane.next --back`)、arrange: Shift+H (`auto.lane.next --back`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `back` | — | `{"type":"boolean"}` | 前へ |

## `auto.lane.remove`

オートメーションレーンを削除

レーンをポイントごと削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.lane.show`

レーンを表示

トラックのオートメーションのレーンを見せます（無ければ足します）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `target` | 必須 | `{"type":"string"}` | volume・pan・mute・send:<id>・param:<id>:<名前> |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |

## `auto.lane.target`

レーンの対象を変更

レーンの対象を変更します（ポイントの値はそのまま）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `to` | 必須 | `{"type":"string"}` | 新しい対象 |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.mode`

オートメーションモード

オートメーションモードを設定します（off・read・touch・latch・write）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: automation: K (`auto.mode --mode touch`)、arrange: K (`auto.mode --mode touch`)、automation: J (`auto.mode --mode read`)、arrange: J (`auto.mode --mode read`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `mode` | 必須 | `{"enum":["off","read","touch","latch","write"],"type":"string"}` | モード |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.point.add`

オートメーションポイントを追加

ポイントを追加します（レーンが無ければ追加し、同じ位置のポイントは値を変更します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `curve` | — | `{"maximum":1.0,"minimum":-1.0,"type":"number"}` | 次のポイントまでのカーブ（-1〜1） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `value` | 必須 | `{"type":"number"}` | 値（dB・-1〜1・0/1・パラメーターの値） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.point.delete`

オートメーションポイントを削除

ポイントを削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `index` | 必須 | `{"maximum":1000000000.0,"minimum":0.0,"type":"integer"}` | ポイントの番号（0 から、時間順） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.point.move`

オートメーションポイントを移動

ポイントを移動します（隣のポイントを押しのけます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 新しい位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `index` | 必須 | `{"maximum":1000000000.0,"minimum":0.0,"type":"integer"}` | ポイントの番号（0 から、時間順） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `value` | — | `{"type":"number"}` | 新しい値 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.point.set`

オートメーションポイントの値を設定

選んだ位置のポイントの値をそろえて設定します（位置は動きません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `ticks` | 必須 | `{"type":"string"}` | ポイントの位置（`960,1920` のように、カンマで区切る） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `value` | 必須 | `{"type":"number"}` | 値 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.point.shape`

オートメーションポイントのカーブを設定

選んだ位置のポイントから次のポイントまでのカーブを設定します（0 で直線。--step で階段。最後のポイントは対象外）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `curve` | 必須 | `{"maximum":1.0,"minimum":-1.0,"type":"number"}` | カーブ（-1〜1） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `step` | — | `{"type":"boolean"}` | 階段にします |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `ticks` | 必須 | `{"type":"string"}` | ポイントの位置（`960,1920` のように、カンマで区切る） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.point.value`

ポイントの値を入力

オートメーションの点の値を入力する枠を開きます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 点の位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `target` | 必須 | `{"type":"string"}` | レーンの対象 |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |

## `auto.points.move`

ポイントを移動

選択したポイントを相対的に移動します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"integer"}` | 移動するtick数 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 対象 |
| `ticks` | 必須 | `{"type":"string"}` | ポイントの位置（カンマ区切り） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `value-by` | — | `{"type":"number"}` | 値の増減 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.segment.shape`

カーブを設定

ポイントから次のポイントまでのカーブを設定します（-1 で下に、1 で上に膨らみます。0 で直線。--step で階段）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `curve` | 必須 | `{"maximum":1.0,"minimum":-1.0,"type":"number"}` | カーブ（-1〜1） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `index` | 必須 | `{"maximum":1000000000.0,"minimum":0.0,"type":"integer"}` | ポイントの番号（0 から、時間順） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `step` | — | `{"type":"boolean"}` | 階段にします（次のポイントまで値を保ち、そこで切り替わります） |
| `target` | 必須 | `{"type":"string"}` | 対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `auto.write`

オートメーションを書き込む

再生中に書き込んだポイントで、レーンの範囲を置き換えます（1 回の再生を 1 回で取り消せます。レーンが無ければ追加します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |
| `writes` | 必須 | `{"items":{"type":"object"},"type":"array"}` | [{track, target, from, to, points: [{tick, value}], fallback}] の配列（from〜to のポイントを points で置き換えます。fallback は、新しいレーンが範囲の外で保つ値で、省くとトラックの今の値です） |

## `bounce.apply`

バウンスした音を置く

バウンスした音をクリップとトラックに置きます（clip.bounce・range.bounce・track.bounce が使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `mute_clips` | — | `{"items":{"type":"string"},"type":"array"}` | ミュートするクリップ（clip の ID のリスト） |
| `mute_tracks` | — | `{"items":{"type":"string"},"type":"array"}` | ミュートするトラック（track の ID のリスト） |
| `replace` | — | `{"items":{"type":"object"},"type":"array"}` | 置き換えるクリップ [{id, file, name?}] |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tracks` | — | `{"items":{"type":"object"},"type":"array"}` | 新しいトラック [{after, like?, name, clips: [{name, file, start, length}]}] |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `browser.favorite`

お気に入りにする

インストゥルメントやエフェクトをお気に入りにします（--off で外します。次に起動しても残ります）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `item` | 必須 | `{"type":"string"}` | インストゥルメントまたはエフェクトの名前（ブラウザーに出る名前） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |

## `browser.fx`

ブラウザーのエフェクトを追加

ブラウザーのエフェクトを、選んだチャンネル・新しいバス・マスターに挿します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `into` | — | `{"default":"channel","enum":["channel","bus","master"],"type":"string"}` | 挿す場所（省略時は選んだチャンネル） |
| `item` | 必須 | `{"type":"string"}` | エフェクトの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `browser.groups`

ブラウザーのグループを開閉

ブラウザーの一覧のグループをすべて開く・畳みます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `state` | 必須 | `{"enum":["open","closed"],"type":"string"}` | 開閉 |

## `browser.hide`

ブラウザーで隠す

インストゥルメントやエフェクトをブラウザーの一覧から隠します（--off で戻します。検索では見つかります）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `item` | 必須 | `{"type":"string"}` | インストゥルメントまたはエフェクトの名前（ブラウザーに出る名前） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |

## `browser.plugin.list`

プラグインの一覧

インストール済みのプラグインを一覧表示します（前回のスキャン結果。plugin.add に渡す値も表示）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `failed` | — | `{"type":"boolean"}` | 読み込めなかったバンドルも含める |
| `kind` | — | `{"enum":["instrument","effect"],"type":"string"}` | インストゥルメントかエフェクトか |
| `limit` | — | `{"maximum":100000.0,"minimum":0.0,"type":"integer"}` | 最大件数（既定は 100） |
| `query` | — | `{"type":"string"}` | 名前・ベンダー・カテゴリーに含まれる文字列 |

## `browser.put`

ファイルをトラックに置く

ファイルを新しいトラックか選んだオーディオトラックに置きます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `into` | — | `{"default":"new","enum":["new","track"],"type":"string"}` | 置く場所（省略時は新しいトラック） |
| `path` | 必須 | `{"type":"string"}` | ファイルのパス |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `browser.search`

ブラウザーで検索

ブラウザーを開いて、検索の文字を入力できるようにします

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+F (`browser.search`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `browser.tab`

ブラウザーのタブ

ブラウザーかインスペクターをその面で開きます（その面が開いていれば閉じます）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: F4 (`browser.tab --tab inspector`)、global: F6 (`browser.tab --tab instruments`)、global: F7 (`browser.tab --tab effects`)、global: F9 (`browser.tab --tab files`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `tab` | 必須 | `{"enum":["instruments","effects","files","song_files","inspector"],"type":"string"}` | 面 |

## `browser.use`

ブラウザーのインストゥルメントを使う

ブラウザーのインストゥルメントを、新しいトラックか選んだトラックで使います

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `into` | — | `{"default":"new","enum":["new","track"],"type":"string"}` | 使う場所（省略時は新しいトラック） |
| `item` | 必須 | `{"type":"string"}` | インストゥルメントの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `cc.draw`

コントローラーに形を描く

コントローラーレーンの範囲に、形をグリッドごとの階段で描きます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `amp` | — | `{"type":"number"}` | 波の振幅（矩形波・三角波・ノコギリ波・サイン波。省略時は範囲の 1/4） |
| `clip` | 必須 | `{"type":"string"}` | MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end-value` | — | `{"type":"number"}` | 終了位置の値（省略時は開始位置と同じ） |
| `from` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `lane` | 必須 | `{"type":"string"}` | レーン: cc1・cc11・cc64（CC の番号）・bend・at |
| `period` | — | `{"maximum":1000000000.0,"minimum":1.0,"type":"integer"}` | 波の 1 周期の長さ（tick。省略時は 1 拍） |
| `phase` | — | `{"type":"integer"}` | 波の位相のずれ（tick。省略時は 0） |
| `points` | — | `{"items":{"type":"object"},"type":"array"}` | フリーハンドの軌跡 [{tick, value}] の配列 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `shape` | 必須 | `{"enum":["free","line","parabola","square","triangle","saw","sine"],"type":"string"}` | 形 |
| `step` | — | `{"maximum":1000000000.0,"minimum":1.0,"type":"integer"}` | ポイントの間隔（tick）。フリーハンドはこの間隔で読み直し、省略すると間引く |
| `to` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `value` | — | `{"type":"number"}` | 開始位置の値（フリーハンドでは不要） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `cc.lane.hide`

レーンを隠す

ピアノロールのレーンを隠します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `index` | 必須 | `{"type":"integer"}` | レーンの位置（0 から） |

## `cc.lane.set`

レーンの種類を変更

ピアノロールのレーンの種類を変えます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `index` | 必須 | `{"type":"integer"}` | レーンの位置（0 から） |
| `lane` | 必須 | `{"type":"string"}` | velocity・bend・at・cc<番号> |

## `cc.point.set`

コントローラーの値を設定

コントローラーレーンのポイントの値を、まとめて同じ値にします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | 必須 | `{"type":"string"}` | MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `lane` | 必須 | `{"type":"string"}` | レーン: cc1・cc11・cc64（CC の番号）・bend・at |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `ticks` | 必須 | `{"type":"string"}` | 対象のポイントの位置（クリップの先頭からの tick。カンマ区切り） |
| `value` | 必須 | `{"type":"integer"}` | 新しい値（レーンの範囲に収めます） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `cc.point.shape`

コントローラーポイントの形を設定

選んだ位置のポイントから次のポイントまでの形を設定します（step は階段、linear は直線。最後のポイントは対象外。CC64 は階段だけ）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | 必須 | `{"type":"string"}` | MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `lane` | 必須 | `{"type":"string"}` | レーン: cc1・cc11・cc64（CC の番号）・bend・at |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `shape` | 必須 | `{"enum":["step","linear"],"type":"string"}` | 次のポイントまでの形 |
| `ticks` | 必須 | `{"type":"string"}` | 対象のポイントの位置（クリップの先頭からの tick。カンマ区切り） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `cc.replace`

コントローラーを置き換え

コントローラーレーンの範囲を、指定したポイントで置き換えます（ポイントが空なら、その範囲を削除します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | 必須 | `{"type":"string"}` | MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"type":"integer"}` | 範囲の開始位置（クリップの先頭からの tick） |
| `lane` | 必須 | `{"type":"string"}` | レーン: cc1・cc11・cc64（CC の番号）・bend・at |
| `points` | — | `{"items":{"type":"object"},"type":"array"}` | [{tick, value}] の配列（tick は範囲内） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `to` | 必須 | `{"type":"integer"}` | 範囲の終了位置（tick。この位置のポイントも置き換えます） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `channel.settings.copy`

チャンネルの設定をコピー

チャンネルの設定（エフェクト・センド・ボリューム・パン）をクリップボードにコピーします

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | コピーするチャンネル（track の ID） |

## `channel.settings.paste`

チャンネルの設定を貼り付け

コピーしたチャンネルの設定を、ほかのチャンネルへ貼り付けます（エフェクトは置き換えます。--from で、クリップボードを通さずに別のチャンネルから写します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | — | `{"type":"string"}` | 設定の元のチャンネル（省略時はクリップボード）（track の ID） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 貼り付け先のチャンネル（track の ID のリスト） |
| `only` | — | `{"type":"string"}` | 貼り付けるもの（inserts, sends, volume, pan をコンマ区切り。省略時はすべて） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.add.audio`

オーディオクリップを作成

オーディオクリップを作成します（オーディオトラックに配置）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 開始位置（小節.拍.tick または tick 数）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `bars` | — | `{"type":"number"}` | 長さ（小節。--length を省略したとき。省略時は 1） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | 必須 | `{"type":"string"}` | オーディオファイル（曲のフォルダーからの相対パスまたは絶対パス） |
| `length` | — | `{"type":"integer"}` | 長さ（tick） |
| `name` | — | `{"type":"string"}` | 名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | 必須 | `{"type":"string"}` | 配置先のトラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.add.midi`

MIDI クリップを作成

MIDI クリップを作成します（インストゥルメントトラックに配置）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: Mod+P (`clip.add.midi`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 開始位置（小節.拍.tick または tick 数）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `bars` | — | `{"type":"number"}` | 長さ（小節。--length を省略したとき。省略時は 1） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `length` | — | `{"type":"integer"}` | 長さ（tick） |
| `name` | — | `{"type":"string"}` | 名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | 必須 | `{"type":"string"}` | 配置先のトラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.bounce`

クリップをバウンス

クリップを、伸縮・速さ・トランスポーズ・逆再生を焼いた 1 つの音に置き換えます（ゲインとフェードはそのまま）。--new-track は、インサートも通した音を下の新しいトラックに置き、元のクリップをミュートします。MIDI のクリップはいつも新しいトラックに置きます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `arrange`。

キー: arrange: Mod+B (`clip.bounce`)、arrange: Mod+Alt+B (`clip.bounce --new-track`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `new-track` | — | `{"type":"boolean"}` | 新しいトラックに置く |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.color.set`

クリップの色を設定

クリップの色を設定します（#rrggbb。none でトラックの色に戻す）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `color` | 必須 | `{"type":"string"}` | #rrggbb または none |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.delete`

クリップを削除

クリップを削除します（--ripple で同じトラックの後ろのクリップを詰める）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

キー: arrange: Delete / Backspace (`clip.delete`)、automation: Delete / Backspace (`clip.delete`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `automation` | — | `{"enum":["follow","stay"],"type":"string"}` | リップル編集で動くクリップの範囲のオートメーション: follow で一緒に動かす（既定）、stay でその場に残す |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `ripple` | — | `{"type":"boolean"}` | リップル編集: 同じトラックの後ろのクリップを詰める・押す（省略時は設定の edit.ripple） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.duplicate`

クリップを複製

クリップを直後に複製します（ノートの ID は新しくなります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: D (`clip.duplicate`)、automation: D (`clip.duplicate`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"integer"}` | 元の位置からのオフセット（tick。指定すると、直後ではなくこの位置に 1 つ複製） |
| `count` | — | `{"default":1,"maximum":256.0,"minimum":1.0,"type":"integer"}` | 複製する数 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `no-overlap` | — | `{"type":"boolean"}` | クリップの重なりを防ぐ: 同じトラックで下になるクリップの重なった部分を切り詰める（真ん中なら 2 つに分ける。省略時は設定の edit.clip.no-overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `rows` | — | `{"type":"integer"}` | 複製先のトラックのずれ（下へのトラック数。負の値で上へ） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.fade.clear`

フェードを削除

オーディオクリップのフェードを外します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.fade.cross`

クロスフェードを作成

重なる（または接する）2 つのオーディオクリップにクロスフェードを作ります。1 つだけ選んだときは右隣、なければ左隣と組みます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: X (`clip.fade.cross`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `curve` | — | `{"type":"number"}` | カーブ（-1〜1。省略時は今の値、新規なら 0.5 の等パワー） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `length` | — | `{"type":"integer"}` | 重ねる長さ（tick。省略時は今の重なり、接していれば短い既定の長さ） |
| `move` | — | `{"type":"integer"}` | クロスフェードの中心を動かす量（tick。正で後ろへ。両方のクリップの境が一緒に動く） |
| `remove` | — | `{"type":"boolean"}` | クロスフェードを外す（クリップの位置は変えない） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.fade.set`

クリップのフェードを設定

オーディオクリップのフェードイン・フェードアウトを設定します（長さは tick、カーブは -1〜1。0 が直線、0.5 が等パワー）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: Shift+X (`clip.fade.set --auto`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `auto` | — | `{"type":"boolean"}` | クリックノイズを避ける短いフェードを両端に付ける（クロスフェード中の端は除く） |
| `clear` | — | `{"type":"boolean"}` | 両方のフェードを外す |
| `curve` | — | `{"type":"number"}` | 両方のカーブ（-1〜1） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `in` | — | `{"type":"integer"}` | フェードインの長さ（tick。0 で外す） |
| `in_curve` | — | `{"type":"number"}` | フェードインのカーブ（-1〜1） |
| `out` | — | `{"type":"integer"}` | フェードアウトの長さ（tick。0 で外す） |
| `out_curve` | — | `{"type":"number"}` | フェードアウトのカーブ（-1〜1） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.gain.point.add`

ゲインの線にポイントを追加

ゲインの線に点を足します（既にその位置に点があれば値を置き換えます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"type":"integer"}` | クリップの先頭からの位置（tick） |
| `db` | — | `{"type":"number"}` | その位置のゲイン（dB。省略時は線のその位置の値） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.gain.point.delete`

ゲインの線のポイントを削除

ゲインの線の点を消します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `index` | 必須 | `{"type":"integer"}` | 点の番号（0 から） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.gain.point.move`

ゲインの線のポイントを移動

ゲインの線の点を動かします（位置は隣の点を越えない範囲に収めます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"type":"integer"}` | 新しい位置（クリップの先頭から tick） |
| `db` | — | `{"type":"number"}` | 新しいゲイン（dB） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `index` | 必須 | `{"type":"integer"}` | 点の番号（0 から。ゲインの線の左から数える） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.gain.reset`

ゲインを戻す

オーディオクリップのゲインを 0 dB に戻し、ゲインの線の点も消します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.gain.set`

クリップのゲインを設定

オーディオクリップのゲイン（dB）を設定します。--db は絶対値、--by は今の値からの変化量。MIN〜MAX の範囲に収めます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: Alt+NumPad+ (`clip.gain.set --by 1`)、arrange: Alt+NumPad- (`clip.gain.set --by -1`)、arrange: Alt+Shift+NumPad+ (`clip.gain.set --by 0.1`)、arrange: Alt+Shift+NumPad- (`clip.gain.set --by -0.1`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"number"}` | 変化量（dB） |
| `db` | — | `{"type":"number"}` | ゲイン（dB） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `reset` | — | `{"type":"boolean"}` | 0 dB に戻し、ゲインの線の点も消す |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.import`

音声ファイルをインポート…

音声ファイル（WAV・AIFF・FLAC・MP3・Ogg Vorbis）をインポートし、オーディオクリップとして置きます。--track を省くと新しいオーディオトラックを作ります。ファイルの長さは変えません（1 回で取り消せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `arrange`。

キー: global: Mod+Shift+O (`clip.import`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 置く位置（省略時は曲の頭）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | 必須 | `{"type":"string"}` | 音声ファイル（絶対パスまたは曲のフォルダーからの相対パス） |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `into` | — | `{"default":"root","type":"string"}` | 新しいトラックの追加先のフォルダーの ID（root または省略時は最上位） |
| `name` | — | `{"type":"string"}` | 新しいトラックの名前（省略時はファイル名） |
| `new-track` | — | `{"type":"boolean"}` | 新しいオーディオトラックに置く（--track を省いたときと同じ） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tempo` | — | `{"default":"auto","enum":["auto","file","keep"],"type":"string"}` | ファイルのテンポの扱い（auto: 曲にクリップが無いときだけ曲のテンポにする、file: 曲のテンポにする、keep: 変えない） |
| `track` | — | `{"type":"string"}` | 置くオーディオトラック（省略時は新しいトラック）（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.import.batch`

音声ファイルをまとめて配置

インポートした音声ファイルをまとめて配置します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `files` | 必須 | `{"items":{"type":"object"},"type":"array"}` | 配置する音声ファイル |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.import.place`

音声ファイルを配置

インポートした音声ファイルをオーディオクリップとして置きます（clip.import が使います。--track を省くと新しいオーディオトラックを作ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 置く位置（省略時は曲の頭）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `bpm` | — | `{"maximum":999.0,"minimum":1.0,"type":"number"}` | 曲の頭のテンポをこの BPM にする（1〜999） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | 必須 | `{"type":"string"}` | 音声ファイル（曲のフォルダーからの相対パスまたは絶対パス） |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `into` | — | `{"default":"root","type":"string"}` | 新しいトラックの追加先のフォルダーの ID（root または省略時は最上位） |
| `length` | 必須 | `{"type":"integer"}` | クリップの長さ（tick） |
| `name` | — | `{"type":"string"}` | 新しいトラックの名前（省略時はファイル名） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tempo` | — | `{"maximum":999.0,"minimum":1.0,"type":"number"}` | クリップのテンポ（BPM。ファイルから分かったとき。省略時は bpm） |
| `track` | — | `{"type":"string"}` | 置くオーディオトラック（省略時は新しいトラック）（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.join`

クリップを結合

同じトラックのクリップを結合します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: G (`clip.join`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.list`

クリップの一覧

クリップを一覧表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `track` | — | `{"type":"string"}` | このトラックのクリップのみ（track の ID） |

## `clip.loop.set`

クリップのループを設定

クリップの中身のループを設定します。クリップはループの終わりまで鳴らしたあと、クリップの終わりまでループを繰り返します（オーディオと MIDI）。--start だけなら終わりを保ち、--length だけなら頭を保ちます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clear` | — | `{"type":"boolean"}` | ループを外す |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `length` | — | `{"type":"integer"}` | ループの長さ（tick） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `start` | — | `{"type":"integer"}` | ループの頭（クリップの頭からの tick） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.move`

クリップを移動

クリップを移動します。--at で開始位置を指定、--by で相対移動、--track で別のトラックへ移動

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 移動先の位置（先頭のクリップをこの位置へ。ほかのクリップも同じ量だけ移動）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `automation` | — | `{"enum":["follow","stay"],"type":"string"}` | クリップ範囲のオートメーション: follow で一緒に移動（既定）、stay でその場に残す。別のトラックへはボリューム・パン・ミュートのみ移動 |
| `by` | — | `{"type":"integer"}` | 移動量（tick。負の値で前へ） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `no-overlap` | — | `{"type":"boolean"}` | クリップの重なりを防ぐ: 同じトラックで下になるクリップの重なった部分を切り詰める（真ん中なら 2 つに分ける。省略時は設定の edit.clip.no-overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `rows` | — | `{"type":"integer"}` | 下へ移動するトラック数（負の値で上へ。各クリップが同じトラック数だけ移動） |
| `track` | — | `{"type":"string"}` | 移動先のトラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.move.cursor`

再生位置へ移動

クリップを再生位置へ移動します（先頭のクリップを --at へ。ほかのクリップも同じ量だけ移動）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: Mod+L (`clip.move.cursor`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 再生位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | 必須 | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.mute`

クリップをミュート

クリップのミュートを切り替えます（--on / --off。省略時は現在の状態を反転）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `mute`。

キー: arrange: Shift+M (`clip.mute`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.normalize`

ノーマライズ

オーディオクリップのゲインを、鳴らす範囲のピークが --peak（既定 0 dB）になるように設定します（素材は書き換えません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `arrange`。

キー: arrange: Alt+N (`clip.normalize`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `peak` | — | `{"default":0.0,"maximum":0.0,"minimum":-60.0,"type":"number"}` | ピークの目標（dBFS） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.normalize.apply`

クリップをノーマライズ

オーディオクリップのゲインを設定します（clip.normalize が使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clips` | 必須 | `{"items":{"type":"object"},"type":"array"}` | クリップ [{id, gain_db}] |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.nudge`

クリップをずらす

クリップをグリッド 1 つ分（--unit bar なら 1 小節）、前か後ろへずらします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: Alt+Right (`clip.nudge --dir right`)、arrange: Alt+Left (`clip.nudge --dir left`)、arrange: Mod+Alt+Right (`clip.nudge --dir right --unit bar`)、arrange: Mod+Alt+Left (`clip.nudge --dir left --unit bar`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dir` | 必須 | `{"enum":["left","right"],"type":"string"}` | 向き |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `grid` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | グリッド（既定は 1/16）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |
| `ids` | 必須 | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `unit` | — | `{"enum":["grid","bar"],"type":"string"}` | ずらす量の単位（既定は grid） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.open`

エディターを表示

エディターを表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `arrange`。

キー: global: F2 (`clip.open`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `clip.quantize`

クリップをクオンタイズ

クリップの開始位置をグリッドにクオンタイズします（--strength で強さを指定。クリップの下のオートメーションも一緒に動きます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: Q (`clip.quantize`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `grid` | — | `{"default":"1/16","oneOf":[{"type":"integer"},{"type":"string"}]}` | グリッド（1/16・1/8t・1/4d・tick 数）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `strength` | — | `{"default":100,"maximum":100.0,"minimum":0.0,"type":"integer"}` | クオンタイズの強さ %（100 で完全にグリッドに揃える） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.relink`

音声ファイルの参照を付け替え

曲が使う音声ファイルの参照を付け替えます（クリップ・フリーズ・機器のサンプル。保存が、曲のフォルダーに集めたファイルへ向けるのに使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `disk` | — | `{"items":{"type":"object"},"type":"array"}` | 一緒に済ませたディスクの名前の変更 [{from, to}]（file.rename が使います。取り消すと名前を戻し、やり直すとまた変えます） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `files` | 必須 | `{"items":{"type":"object"},"type":"array"}` | 付け替える参照 [{from, to}] |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.rename`

クリップ名を変更

クリップの名前を変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `name` | 必須 | `{"type":"string"}` | 新しい名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.replace`

クリップの素材を差し替え

クリップの素材を差し替えます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | 必須 | `{"type":"string"}` | 素材のファイル |
| `id` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.resize`

クリップの長さを変更

クリップの長さを変更します（中のノートは削除しません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `adjacent` | — | `{"type":"boolean"}` | 終了位置に接している次のクリップの開始位置も同じ量だけ動かす（境目だけが動き、隙間も重なりもできない） |
| `automation` | — | `{"enum":["follow","stay"],"type":"string"}` | リップル編集で動くクリップの範囲のオートメーション: follow で一緒に動かす（既定）、stay でその場に残す |
| `by` | — | `{"type":"integer"}` | 伸ばす量（tick。負の値で縮める） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 新しい終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `length` | — | `{"type":"integer"}` | 新しい長さ（tick） |
| `no-overlap` | — | `{"type":"boolean"}` | クリップの重なりを防ぐ: 同じトラックで下になるクリップの重なった部分を切り詰める（真ん中なら 2 つに分ける。省略時は設定の edit.clip.no-overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `ripple` | — | `{"type":"boolean"}` | リップル編集: 同じトラックの後ろのクリップを詰める・押す（省略時は設定の edit.ripple） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.reverse`

逆再生

オーディオクリップを逆再生にします（素材は書き換えません）。クリップは素材の同じ範囲を後ろから鳴らします。--on / --off を省くと切り替えます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `arrange`。

キー: arrange: Mod+R (`clip.reverse`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.reverse.apply`

クリップを逆再生

オーディオクリップの逆再生と素材の開始位置を設定します（clip.reverse が使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clips` | 必須 | `{"items":{"type":"object"},"type":"array"}` | クリップ [{id, reverse, offset}] |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.slip`

クリップの中身をずらす

クリップの枠を動かさず、中身（オーディオの読み出し位置）だけをずらします。--by が正なら中身が後ろへ動く

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | 必須 | `{"type":"integer"}` | 中身を動かす量（tick。正で後ろへ、負で前へ） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.speed.set`

クリップの速さを設定

オーディオクリップの速さ（%）を設定します。音の高さは変わりません。素材の同じ範囲を保って長さが変わります

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `percent` | 必須 | `{"maximum":200.0,"minimum":50.0,"type":"number"}` | 速さ（%） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.split`

クリップを分割

クリップを指定位置で分割します（左は元の ID のまま、右は新しい ID。位置をまたぐノートは両方に分かれます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `cut`。

キー: arrange: Alt+X (`clip.split`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 分割する位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.split.grid`

グリッドで分割

クリップをグリッドの線ごとに分割します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: Mod+G (`clip.split.grid`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `grid` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | グリッド（既定は 1/4）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |
| `ids` | 必須 | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.split.range`

範囲の両端で分割

範囲の両端でクリップを分割します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

キー: arrange: Mod+Shift+X (`clip.split.range --range loop`)、arrange: Mod+Alt+X (`clip.split.range`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `loop` | — | `{"type":"boolean"}` | ループ範囲を使う（--range loop と同じ） |
| `range` | — | `{"type":"string"}` | loop・開始..終了・section:名前（start・end・loop の代わり） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `start` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.stretch`

曲のテンポに合わせる

オーディオクリップを曲のテンポに合わせるか（伸縮するか）を設定します。素材のテンポが無いときは、クリップの位置の曲のテンポを素材のテンポにします。--follow を省くと切り替えます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `follow` | — | `{"enum":["on","off"],"type":"string"}` | 曲のテンポに合わせる |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.tempo.detect`

テンポを検出

オーディオクリップの鳴らす範囲から素材のテンポを検出し、クリップのテンポにします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.tempo.detect.apply`

検出したテンポを設定

オーディオクリップごとに素材のテンポを設定します（clip.tempo.detect が使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clips` | 必須 | `{"items":{"type":"object"},"type":"array"}` | クリップ [{id, bpm}] |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.tempo.set`

クリップのテンポを設定

オーディオクリップの素材のテンポ（bpm）を設定します。曲のテンポに合わせているクリップは、素材の同じ範囲を保って長さが変わります

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bpm` | 必須 | `{"maximum":999.0,"minimum":1.0,"type":"number"}` | 素材のテンポ（bpm） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.transpose`

クリップをトランスポーズ

クリップの音の高さを半音で設定します（長さは変わりません）。オーディオのクリップは鳴らす高さが、MIDI のクリップはノートに足す値が変わります。--by は今の値からの変化量。範囲（−24〜24）の外は丸めます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"integer"}` | 変化量（半音） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `semis` | — | `{"maximum":24.0,"minimum":-24.0,"type":"integer"}` | トランスポーズ（半音。範囲の外は丸めます） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.transpose.write`

トランスポーズをノートに反映

MIDI クリップのトランスポーズをノートの高さに反映し、値を 0 に戻します（鳴る高さは変わりません）。値が 0 のクリップはそのままです。反映した後の高さが 0〜127 の外に出るノートがあると、何も変えずに失敗します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.trim`

クリップの開始位置をトリム

クリップの開始位置をトリムします（終了位置は固定。中身は曲の同じ位置に残ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | 必須 | `{"type":"integer"}` | 開始位置の移動量（tick。正の値で縮め、負の値で前へ伸ばす） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `no-overlap` | — | `{"type":"boolean"}` | クリップの重なりを防ぐ: 同じトラックで下になるクリップの重なった部分を切り詰める（真ん中なら 2 つに分ける。省略時は設定の edit.clip.no-overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `clip.tune`

クリップのピッチを微調整

オーディオクリップの音の高さをセントで微調整します（長さは変わりません）。--by は今の値からの変化量

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `arrange`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"integer"}` | 変化量（セント） |
| `cents` | — | `{"maximum":100.0,"minimum":-100.0,"type":"integer"}` | 微調整（セント） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `edit.clipboard`

クリップボードの中身

クリップボードにあるものを表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `edit.copy`

コピー

ノート・クリップ・レーンのポイントをクリップボードにコピーします

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: mixer: Mod+C (`edit.copy`)、edit: Mod+C (`edit.copy`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | — | `{"type":"string"}` | コントローラーのポイントを持つ MIDI クリップ（clip の ID） |
| `from` | — | `{"type":"integer"}` | レーンのポイントの範囲の開始（tick。この位置のポイントも含みます） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート・クリップ・トラック（省略時は選択中のもの。トラックだけのときはトラックごと）（note\|clip\|track の ID のリスト） |
| `lane` | — | `{"type":"string"}` | コントローラーのレーン: cc1・cc64・bend・at（--clip と組み合わせます） |
| `plugin` | — | `{"type":"string"}` | チャンネルのインサートのエフェクト（複製は、すぐ後ろに入ります）（plugin の ID） |
| `target` | — | `{"type":"string"}` | オートメーションの対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `ticks` | — | `{"type":"string"}` | レーンのポイントの位置（カンマ区切りの tick。コントローラーはクリップの先頭から、オートメーションは曲の先頭から） |
| `to` | — | `{"type":"integer"}` | レーンのポイントの範囲の終了（tick。この位置のポイントも含みます） |
| `track` | — | `{"type":"string"}` | オートメーションのトラック（track の ID） |

## `edit.cut`

切り取り

ノート・クリップ・レーンのポイントを切り取り、クリップボードに置きます（クリップは --ripple で同じトラックの後ろを詰める）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: mixer: Mod+X (`edit.cut`)、edit: Mod+X (`edit.cut`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `automation` | — | `{"enum":["follow","stay"],"type":"string"}` | リップル編集で動くクリップの範囲のオートメーション: follow で一緒に動かす（既定）、stay でその場に残す |
| `clip` | — | `{"type":"string"}` | コントローラーのポイントを持つ MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | — | `{"type":"integer"}` | レーンのポイントの範囲の開始（tick。この位置のポイントも含みます） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート・クリップ・トラック（省略時は選択中のもの。トラックだけのときはトラックごと）（note\|clip\|track の ID のリスト） |
| `lane` | — | `{"type":"string"}` | コントローラーのレーン: cc1・cc64・bend・at（--clip と組み合わせます） |
| `plugin` | — | `{"type":"string"}` | チャンネルのインサートのエフェクト（複製は、すぐ後ろに入ります）（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `ripple` | — | `{"type":"boolean"}` | リップル編集: 同じトラックの後ろのクリップを詰める・押す（省略時は設定の edit.ripple） |
| `target` | — | `{"type":"string"}` | オートメーションの対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `ticks` | — | `{"type":"string"}` | レーンのポイントの位置（カンマ区切りの tick。コントローラーはクリップの先頭から、オートメーションは曲の先頭から） |
| `to` | — | `{"type":"integer"}` | レーンのポイントの範囲の終了（tick。この位置のポイントも含みます） |
| `track` | — | `{"type":"string"}` | オートメーションのトラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `edit.delete`

削除

ノート・クリップ・レーンのポイントを削除します（クリップボードは変えません。クリップは --ripple で同じトラックの後ろを詰める）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

キー: mixer: Delete / Backspace (`edit.delete`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `automation` | — | `{"enum":["follow","stay"],"type":"string"}` | リップル編集で動くクリップの範囲のオートメーション: follow で一緒に動かす（既定）、stay でその場に残す |
| `clip` | — | `{"type":"string"}` | コントローラーのポイントを持つ MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | — | `{"type":"integer"}` | レーンのポイントの範囲の開始（tick。この位置のポイントも含みます） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート・クリップ・トラック（省略時は選択中のもの。トラックだけのときはトラックごと）（note\|clip\|track の ID のリスト） |
| `lane` | — | `{"type":"string"}` | コントローラーのレーン: cc1・cc64・bend・at（--clip と組み合わせます） |
| `plugin` | — | `{"type":"string"}` | チャンネルのインサートのエフェクト（複製は、すぐ後ろに入ります）（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `ripple` | — | `{"type":"boolean"}` | リップル編集: 同じトラックの後ろのクリップを詰める・押す（省略時は設定の edit.ripple） |
| `target` | — | `{"type":"string"}` | オートメーションの対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `ticks` | — | `{"type":"string"}` | レーンのポイントの位置（カンマ区切りの tick。コントローラーはクリップの先頭から、オートメーションは曲の先頭から） |
| `to` | — | `{"type":"integer"}` | レーンのポイントの範囲の終了（tick。この位置のポイントも含みます） |
| `track` | — | `{"type":"string"}` | オートメーションのトラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `edit.duplicate`

複製

ノート・クリップ・レーンのポイントを複製します（クリップボードは変えません。既定は、選んだものの幅だけ右）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"integer"}` | 元の位置からのずれ（tick。省略時は、選んだものの幅だけ右） |
| `clip` | — | `{"type":"string"}` | コントローラーのポイントを持つ MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | — | `{"type":"integer"}` | レーンのポイントの範囲の開始（tick。この位置のポイントも含みます） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート・クリップ・トラック（省略時は選択中のもの。トラックだけのときはトラックごと）（note\|clip\|track の ID のリスト） |
| `lane` | — | `{"type":"string"}` | コントローラーのレーン: cc1・cc64・bend・at（--clip と組み合わせます） |
| `no-overlap` | — | `{"type":"boolean"}` | クリップの重なりを防ぐ: 同じトラックで下になるクリップの重なった部分を切り詰める（真ん中なら 2 つに分ける。省略時は設定の edit.clip.no-overlap） |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `plugin` | — | `{"type":"string"}` | チャンネルのインサートのエフェクト（複製は、すぐ後ろに入ります）（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | — | `{"type":"string"}` | オートメーションの対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `ticks` | — | `{"type":"string"}` | レーンのポイントの位置（カンマ区切りの tick。コントローラーはクリップの先頭から、オートメーションは曲の先頭から） |
| `to` | — | `{"type":"integer"}` | レーンのポイントの範囲の終了（tick。この位置のポイントも含みます） |
| `track` | — | `{"type":"string"}` | オートメーションのトラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `edit.nudge.left`

グリッドの前へ

グリッドの前へ

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: automation: Alt+Left (`edit.nudge.left`)、roll: Alt+Left (`edit.nudge.left`)、roll: Left (`edit.nudge.left --grid 1/16`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `grid` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 動かす幅（無ければ今のグリッド）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |

## `edit.nudge.right`

グリッドの後ろへ

グリッドの後ろへ

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: automation: Alt+Right (`edit.nudge.right`)、roll: Alt+Right (`edit.nudge.right`)、roll: Right (`edit.nudge.right --grid 1/16`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `grid` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 動かす幅（無ければ今のグリッド）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |

## `edit.paste`

貼り付け

クリップボードのものを貼り付けます（貼り付けたものの ID は新しくなります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: mixer: Mod+V (`edit.paste`)、edit: Mod+V (`edit.paste`)、edit: Mod+Shift+V (`edit.paste --at original`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 貼り付ける位置（最初のものがここに来ます。省略か original で、元の位置）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `clip` | — | `{"type":"string"}` | 貼り付け先の MIDI クリップ（ノートとコントローラーのポイント。省略時は取った元のクリップ）（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `lane` | — | `{"type":"string"}` | 貼り付け先のコントローラーのレーン（省略時は取った元のレーン） |
| `no-overlap` | — | `{"type":"boolean"}` | クリップの重なりを防ぐ: 同じトラックで下になるクリップの重なった部分を切り詰める（真ん中なら 2 つに分ける。省略時は設定の edit.clip.no-overlap） |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | — | `{"type":"string"}` | オートメーションの対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>） |
| `track` | — | `{"type":"string"}` | 貼り付け先のトラック（クリップは最初のクリップの行をここに合わせ、他は相対的にずらします。オートメーションのポイントもここへ。エフェクトはインサートの末尾へ）（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `edit.ripple`

リップル編集

リップル編集を切り替えます（クリップの削除・切り取り・範囲の削除・長さの変更で、同じトラックの後ろのクリップを詰める・押す。省略時は現在の状態を切り替え）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

キー: global: Mod+Alt+R (`edit.ripple`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `engine.stats`

エンジンの負荷

オーディオエンジンの全体の処理の負荷と、間に合わなかった回数を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `gauge`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `export.cancel`

書き出しをキャンセル

書き出しまたはプラグインのスキャンをキャンセルします（書き出しはファイルを書き出しません。別のプロセスの書き出しもキャンセルできます。スキャンは読み込み済みの分を一覧に残します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `job` | 必須 | `{"type":"string"}` | ジョブの id |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.dawproject`

DAWproject として書き出す…

曲を DAWproject（.dawproject）に書き出します。トラック・クリップ・センド・オートメーション・テンポ・拍子と、クリップの音のファイルを含みます。同じ名前のファイルはごみ箱に移動してから書き出し、含めなかったものを一覧にして返します

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `file`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `external` | — | `{"type":"boolean"}` | 音のファイルを中に入れず、今の場所を指す（ファイルは軽くなるが、渡すときに音のファイルも要る） |
| `out` | — | `{"type":"string"}` | 書き出すファイル（省略時は曲のフォルダーの「曲名.dawproject」） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.delete`

書き出したファイルを削除

書き出したファイルをごみ箱に移動し、一覧から削除します

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `job` | 必須 | `{"type":"string"}` | 書き出しジョブの id |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.import`

書き出した音を取り込む

書き出したファイルを新しいトラックへ取り込みます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 配置する位置（省略時は書き出した範囲の頭）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | — | `{"type":"string"}` | 取り込むファイル（名前かパス。省略時は書き出したすべてのファイル） |
| `job` | 必須 | `{"type":"string"}` | 書き出しジョブの id |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.midi`

MIDI ファイルとして書き出す…

インストゥルメントトラックのノートを MIDI ファイル（.mid、フォーマット 1）に書き出します。曲名・テンポ・拍子とトラック名を含みます

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `midi`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clips` | — | `{"items":{"type":"string"},"type":"array"}` | このクリップのトラックと範囲を書き出す（clip の ID のリスト） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `each` | — | `{"type":"boolean"}` | トラックごとに別のファイルにする（省略時は 1 つのファイルにまとめる） |
| `from` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | range が custom のときの開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `name` | — | `{"type":"string"}` | ファイル名のテンプレート。{曲} は曲名、{トラック} はトラックの名前（省略時は前回のテンプレート。初回は {曲}、--each のときは {曲}_{トラック}） |
| `numbered` | — | `{"type":"boolean"}` | 同じ名前のファイルがあれば、番号を付けた名前で書き出す（省略時は前のファイルをごみ箱に移動し、同じ名前で書き出して一覧の行を置き換える） |
| `out` | — | `{"type":"string"}` | 書き出すファイル（--each のときはフォルダー。省略時は曲のフォルダーの MIDI） |
| `overwrite` | — | `{"type":"boolean"}` | 同じ名前のファイルがあれば、その場で上書きする（前のファイルをごみ箱に移動しない） |
| `range` | — | `{"type":"string"}` | 書き出す範囲（song・loop・section:名前または id・3.1.0..5.1.0 のような位置の範囲。省略時は曲全体、--clips を指定したときはそのクリップの範囲） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `section` | — | `{"type":"string"}` | range が section のときのセクション名または id |
| `to` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | range が custom のときの終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `tracks` | — | `{"items":{"type":"string"},"type":"array"}` | 書き出すインストゥルメントトラック（省略時は --clips のトラック、それも無ければすべてのインストゥルメントトラック）（track の ID のリスト） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.mixdown`

ミックスダウンを書き出す…

曲全体を 1 つの WAV・FLAC・MP3 ファイルにミックスダウンします（画面を使わず、再生と同じエンジンでレンダリングし、完了後にラウドネスとトゥルーピークを表示します）

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `export`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `background` | — | `{"type":"boolean"}` | バックグラウンドで書き出し、すぐにジョブの id を返す（進行状況は export.status） |
| `bit_depth` | — | `{"maximum":32.0,"minimum":16.0,"type":"integer"}` | ビット深度 16・24・32（32 は浮動小数点。省略時は曲の設定。MP3 では使いません） |
| `bitrate` | — | `{"maximum":320.0,"minimum":128.0,"type":"integer"}` | MP3 のビットレート（kbps、固定）128・192・256・320（省略時は 320） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `format` | — | `{"enum":["wav","flac","mp3"],"type":"string"}` | ファイル形式（省略時は --out の拡張子、無ければ wav。FLAC は 16・24 bit のみ。MP3 は 48 kHz までで、--sample_rate を省略すると曲が 48 kHz を超えるときは 48 kHz（44.1 kHz の倍数なら 44.1 kHz）。MP3 は曲名・トラック名をタグに入れます） |
| `from` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | range が custom のときの開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `name` | — | `{"type":"string"}` | ファイル名のテンプレート。{曲} は曲名、{トラック} はトラック・バスの名前（省略時は前回のテンプレート。初回はミックスダウンが {曲}、ステムが {曲}_{トラック}。ステムのテンプレートには {トラック} が必要です） |
| `numbered` | — | `{"type":"boolean"}` | 同じ名前のファイルがあれば、番号を付けた名前で書き出す（省略時は前のファイルをごみ箱に移動し、同じ名前で書き出して一覧の行を置き換える） |
| `out` | — | `{"type":"string"}` | 書き出し先のファイル（省略時は曲のフォルダーの Mixdown/ に --name のテンプレートで書き出し） |
| `overwrite` | — | `{"type":"boolean"}` | 同じ名前のファイルがあれば、その場で上書きする（前のファイルをごみ箱に移動しない） |
| `range` | — | `{"default":"song","type":"string"}` | 書き出す範囲（song = 曲の先頭から最後のクリップの終わりまで、loop = ループ範囲、section:名前または id（カンマ区切りで複数）、3.1.0..5.1.0 のような位置の範囲。section と custom は --section・--from・--to と組み合わせる古い書き方） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `sample_rate` | — | `{"maximum":384000.0,"minimum":8000.0,"type":"integer"}` | サンプルレート（省略時は曲の設定） |
| `section` | — | `{"type":"string"}` | range が section のときのセクション名または id（カンマ区切りで複数指定すると、最初のセクションの開始から最後のセクションの終了まで） |
| `tail` | — | `{"maximum":600.0,"minimum":0.0,"type":"number"}` | 範囲の後ろに追加する秒数（エフェクトの余韻用。既定は 0） |
| `to` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | range が custom のときの終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.open`

書き出しの設定

書き出しの設定を返します（--job を指定するとそのジョブと同じ設定、省略時は既定の設定と前回のファイル名のテンプレート）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+E (`export.open`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `job` | — | `{"type":"string"}` | 設定をコピーする書き出しジョブの id |

## `export.preview`

書き出した音を聴く

書き出したファイルを試聴します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `file` | — | `{"type":"string"}` | 聴くファイル（名前かパス。省略時は書き出したすべてのファイル） |
| `job` | 必須 | `{"type":"string"}` | 書き出しジョブの id |
| `output` | — | `{"type":"string"}` | 出力（device・silent。省略時は実行中の出力設定） |
| `status` | — | `{"type":"boolean"}` | 試聴の状態を取得 |
| `stop` | — | `{"type":"boolean"}` | 試聴を停止 |

## `export.reveal_os`

書き出したファイルを表示

書き出したファイルをエクスプローラー（Finder）で表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `file` | — | `{"type":"string"}` | 表示するファイル（名前かパス。省略時は最初のファイル） |
| `job` | 必須 | `{"type":"string"}` | 書き出しジョブの id |

## `export.selection`

選択範囲を書き出す…

選んだクリップの先頭から末尾までを書き出します

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `background` | — | `{"type":"boolean"}` | バックグラウンドで書き出し、すぐにジョブの id を返す（進行状況は export.status） |
| `bit_depth` | — | `{"maximum":32.0,"minimum":16.0,"type":"integer"}` | ビット深度 16・24・32（32 は浮動小数点。省略時は曲の設定。MP3 では使いません） |
| `bitrate` | — | `{"maximum":320.0,"minimum":128.0,"type":"integer"}` | MP3 のビットレート（kbps、固定）128・192・256・320（省略時は 320） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `format` | — | `{"enum":["wav","flac","mp3"],"type":"string"}` | ファイル形式（省略時は --out の拡張子、無ければ wav。FLAC は 16・24 bit のみ。MP3 は 48 kHz までで、--sample_rate を省略すると曲が 48 kHz を超えるときは 48 kHz（44.1 kHz の倍数なら 44.1 kHz）。MP3 は曲名・トラック名をタグに入れます） |
| `from` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | range が custom のときの開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `ids` | 必須 | `{"items":{"type":"string"},"type":"array"}` | 書き出す範囲を決めるクリップ（clip の ID のリスト） |
| `name` | — | `{"type":"string"}` | ファイル名のテンプレート。{曲} は曲名、{トラック} はトラック・バスの名前（省略時は前回のテンプレート。初回はミックスダウンが {曲}、ステムが {曲}_{トラック}。ステムのテンプレートには {トラック} が必要です） |
| `numbered` | — | `{"type":"boolean"}` | 同じ名前のファイルがあれば、番号を付けた名前で書き出す（省略時は前のファイルをごみ箱に移動し、同じ名前で書き出して一覧の行を置き換える） |
| `out` | — | `{"type":"string"}` | 書き出し先のファイル（省略時は曲のフォルダーの Mixdown/ に --name のテンプレートで書き出し） |
| `overwrite` | — | `{"type":"boolean"}` | 同じ名前のファイルがあれば、その場で上書きする（前のファイルをごみ箱に移動しない） |
| `range` | — | `{"default":"song","type":"string"}` | 書き出す範囲（song = 曲の先頭から最後のクリップの終わりまで、loop = ループ範囲、section:名前または id（カンマ区切りで複数）、3.1.0..5.1.0 のような位置の範囲。section と custom は --section・--from・--to と組み合わせる古い書き方） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `sample_rate` | — | `{"maximum":384000.0,"minimum":8000.0,"type":"integer"}` | サンプルレート（省略時は曲の設定） |
| `section` | — | `{"type":"string"}` | range が section のときのセクション名または id（カンマ区切りで複数指定すると、最初のセクションの開始から最後のセクションの終了まで） |
| `tail` | — | `{"maximum":600.0,"minimum":0.0,"type":"number"}` | 範囲の後ろに追加する秒数（エフェクトの余韻用。既定は 0） |
| `to` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | range が custom のときの終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.status`

書き出しの状態

書き出しとプラグインのスキャンの進行状況と結果を表示します（--job を省略するとすべて）。保存済みの曲の書き出しは曲のフォルダーの exports/ に記録され、別のプロセスからも参照でき、異常終了したものは interrupted（中断）になります

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `job` | — | `{"type":"string"}` | ジョブの id（export.mixdown・export.stems・plugin.rescan が返すもの） |

## `export.stems`

ステムをファイルへ書き出す…

トラックとバスを 1 つずつ WAV・FLAC・MP3 ファイルにステムとして書き出します（ミュート中のものはスキップして通知します）

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `layers`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `background` | — | `{"type":"boolean"}` | バックグラウンドで書き出し、すぐにジョブの id を返す（進行状況は export.status） |
| `bit_depth` | — | `{"maximum":32.0,"minimum":16.0,"type":"integer"}` | ビット深度 16・24・32（32 は浮動小数点。省略時は曲の設定。MP3 では使いません） |
| `bitrate` | — | `{"maximum":320.0,"minimum":128.0,"type":"integer"}` | MP3 のビットレート（kbps、固定）128・192・256・320（省略時は 320） |
| `dir` | — | `{"type":"string"}` | 書き出し先のフォルダー（省略時は曲のフォルダーの Stems/） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `folders` | — | `{"items":{"type":"string"},"type":"array"}` | フォルダーの id。フォルダーにつないだバスをフォルダー名の 1 本のステムにする（バスにつないでいないフォルダーは書き出せません。指定すると what は selected）（folder の ID のリスト） |
| `format` | — | `{"enum":["wav","flac","mp3"],"type":"string"}` | ファイル形式（省略時は --out の拡張子、無ければ wav。FLAC は 16・24 bit のみ。MP3 は 48 kHz までで、--sample_rate を省略すると曲が 48 kHz を超えるときは 48 kHz（44.1 kHz の倍数なら 44.1 kHz）。MP3 は曲名・トラック名をタグに入れます） |
| `from` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | range が custom のときの開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 書き出すトラックとバスの id（指定すると what は selected）（track の ID のリスト） |
| `name` | — | `{"type":"string"}` | ファイル名のテンプレート。{曲} は曲名、{トラック} はトラック・バスの名前（省略時は前回のテンプレート。初回はミックスダウンが {曲}、ステムが {曲}_{トラック}。ステムのテンプレートには {トラック} が必要です） |
| `numbered` | — | `{"type":"boolean"}` | 同じ名前のファイルがあれば、番号を付けた名前で書き出す（省略時は前のファイルをごみ箱に移動し、同じ名前で書き出して一覧の行を置き換える） |
| `overwrite` | — | `{"type":"boolean"}` | 同じ名前のファイルがあれば、その場で上書きする（前のファイルをごみ箱に移動しない） |
| `range` | — | `{"default":"song","type":"string"}` | 書き出す範囲（song = 曲の先頭から最後のクリップの終わりまで、loop = ループ範囲、section:名前または id（カンマ区切りで複数）、3.1.0..5.1.0 のような位置の範囲。section と custom は --section・--from・--to と組み合わせる古い書き方） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `sample_rate` | — | `{"maximum":384000.0,"minimum":8000.0,"type":"integer"}` | サンプルレート（省略時は曲の設定） |
| `section` | — | `{"type":"string"}` | range が section のときのセクション名または id（カンマ区切りで複数指定すると、最初のセクションの開始から最後のセクションの終了まで） |
| `tail` | — | `{"maximum":600.0,"minimum":0.0,"type":"number"}` | 範囲の後ろに追加する秒数（エフェクトの余韻用。既定は 0） |
| `to` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | range が custom のときの終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |
| `what` | — | `{"default":"all","enum":["tracks","buses","all","by_bus","selected"],"type":"string"}` | 書き出す単位（トラックごと・バスごと（バスと、出力先がマスターのトラック）・両方・by_bus・selected（--ids で指定したもの））。tracks = 全トラック、buses = 全バス |

## `export.touch`

書き出したファイルを古いものにする

トラックを変更したものとして、そのトラックを含む書き出したファイルを古いものとして扱います（プラグインの画面での変更など）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tracks` | 必須 | `{"items":{"type":"string"},"type":"array"}` | 変更したトラック（track の ID のリスト） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.update`

もう一度書き出す

書き出したファイルを、現在の曲から同じ設定で書き出し直します（ステムは変わったファイルだけを書き出し直し、ほかのファイルはそのまま残します。書き直すファイルの前の版はごみ箱に移動します）

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `background` | — | `{"type":"boolean"}` | バックグラウンドで書き出し、すぐにジョブの id を返す（進行状況は export.status） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | — | `{"type":"string"}` | 書き出し直すファイル（名前かパス。省略時は変わったファイル。テンポ・拍子・曲の設定・範囲が変わったときはすべて） |
| `job` | 必須 | `{"type":"string"}` | 書き出し直すジョブの id |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `export.watch`

書き出しの古さを知らせる

書き出したファイルが古くなったとき・最新に戻ったときに export.stale のイベントで知らせます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `stop` | — | `{"type":"boolean"}` | 知らせるのをやめる |

## `file.add.folder`

新しいフォルダー

フォルダーの中に新しいフォルダーを作成します

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | 新しいフォルダーの名前 |
| `path` | 必須 | `{"type":"string"}` | 作成する場所のフォルダーのパス（相対パスは曲のフォルダーから） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `file.collect`

曲のフォルダーにコピー

曲のフォルダーの外のファイルを、置き場所の Imports にコピーし、使うクリップと機器をコピーへ付け替えます（1 回で取り消せます。取り消してもコピーは残ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `paths` | — | `{"type":"string"}` | コピーするファイル（file.list の file。いくつも渡すときは \| で区切ります。省略時は曲のフォルダーの外のファイルすべて） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `file.list`

曲が使うファイル

曲が使う音声ファイル（クリップ・フリーズ・機器のサンプルの音）と、使われていない録音を一覧します。ファイルごとに、種類（record・import・bounce・freeze）、大きさ、使うクリップと機器の数、曲のフォルダーの中か外か、見つかるかを表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `file.locate`

ファイルの場所を指定

見つからないファイルの場所を指定し、使うクリップと機器を付け替えます（1 回で取り消せます）。--new_path は同じフォルダーにあったほかの見つからないファイルも、選んだファイルの隣の同じ名前のファイルへ付け替えます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `folder` | — | `{"type":"string"}` | 探すフォルダー（見つからないファイルを、この下から同じ名前で探します。--path を省くと全部） |
| `new_path` | — | `{"type":"string"}` | 付け替える先のファイル（--path と一緒に指定します） |
| `path` | — | `{"type":"string"}` | 見つからないファイル（file.list の file） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `file.rename`

ファイル名を変更

ファイルやフォルダーの名前を変えます。曲が使うファイルは曲の中の参照も新しい名前に付け替えます（1 操作で取り消せ、取り消すとファイルの名前も戻ります）。拡張子を省くと元の拡張子のままです

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | 新しい名前 |
| `path` | 必須 | `{"type":"string"}` | ファイルまたはフォルダーのパス（相対パスは曲のフォルダーから） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `file.reveal_os`

エクスプローラーで表示

ファイルやフォルダーをエクスプローラー（Finder）で表示します（相対パスは曲のフォルダーから。省くと曲のフォルダー）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `path` | — | `{"type":"string"}` | ファイルまたはフォルダーのパス（省略時は曲のフォルダー） |

## `file.trash`

ごみ箱に移動

ファイルやフォルダーを OS のごみ箱に移動します。曲が使うファイルと、それを含むフォルダーは移動できません

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `path` | 必須 | `{"type":"string"}` | ファイルまたはフォルダーのパス（相対パスは曲のフォルダーから） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `folder.add`

フォルダーを追加

フォルダーを追加します（入れ子にでき、バスに接続できます。フォルダー自体は音を出しません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `folder`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `into` | — | `{"default":"root","type":"string"}` | 追加先のフォルダーの ID（root または省略時は最上位） |
| `name` | — | `{"type":"string"}` | 名前（省略時は自動で付けます） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `folder.bus.add`

新しいバスにフォルダーを接続

新しいバスを作成し、フォルダーをそのバスに接続します（1 回の取り消しで両方とも元に戻ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `folder`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | フォルダー（folder の ID） |
| `into` | — | `{"default":"root","type":"string"}` | バスの追加先のフォルダーの ID（root または省略時は最上位） |
| `name` | — | `{"type":"string"}` | バスの名前（省略時は自動で付けます） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `folder.bus.set`

フォルダーをバスに接続

フォルダーをバスに接続します。出力先が既定のままのトラックは、そのバスへ出力されます（none で解除）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `folder`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bus` | 必須 | `{"type":"string"}` | バスの ID または none |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | フォルダー（folder の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `folder.delete`

フォルダーを削除

フォルダーを削除します。中身は 1 つ上の階層へ移動します（--with-tracks で中のトラックごと削除）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | フォルダー（folder の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |
| `with-tracks` | — | `{"type":"boolean"}` | 中のトラックとフォルダーも削除する |

## `folder.duplicate`

フォルダーを複製

フォルダーを中のトラックごと複製します（クリップを含む。ID は新しくなります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `folder`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | フォルダー（folder の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `folder.pack`

フォルダーにまとめる

選んだトラックとフォルダーを、新しいフォルダーにまとめます（先頭のものがあった場所に作ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `folder`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | まとめるトラックやフォルダー（track\|folder の ID のリスト） |
| `name` | — | `{"type":"string"}` | フォルダー名（省略時は Folder 1 のような名前） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `folder.rename`

フォルダー名を変更

フォルダーの名前を変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `folder`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | フォルダー（folder の ID） |
| `name` | 必須 | `{"type":"string"}` | 新しい名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `folder.unpack`

まとめを解く

フォルダーのまとめを解除します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `folder`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | フォルダー（folder の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `history.jump`

履歴の手順へ移動

履歴の任意の手順に移動します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `step` | 必須 | `{"type":"integer"}` | 手順の番号（history.list の step） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `history.list`

操作履歴

操作履歴を表示します（ユーザーと AI で共通）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `history`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `limit` | — | `{"maximum":100000.0,"minimum":1.0,"type":"integer"}` | 新しいものからの件数 |

## `history.redo`

やり直す

取り消した操作をやり直します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `redo`。

キー: global: Mod+Y / Mod+Shift+Z (`history.redo`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `count` | — | `{"default":1,"maximum":100000.0,"minimum":1.0,"type":"integer"}` | やり直す操作の数 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `history.undo`

元に戻す

操作を取り消します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `undo`。

キー: global: Mod+Z (`history.undo`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `count` | — | `{"default":1,"maximum":100000.0,"minimum":1.0,"type":"integer"}` | 取り消す操作の数 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `job.cancel`

処理をキャンセル

実行中のバウンスとフリーズをキャンセルします（--id を省略するとすべて）。曲は元のままです

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | — | `{"type":"string"}` | 処理の ID（job.list の id） |

## `job.list`

実行中の処理

実行中のバウンスとフリーズを一覧します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `keymap.conflicts`

キーの衝突

同じキーに複数のコマンドがある箇所と、OS が使うキーとの重なりを一覧します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `keymap.export`

キーの割り当てを書き出す…

キーの割り当てを書き出します（--to を省くと内容を返します）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `format` | — | `{"default":"toml","enum":["toml","json","html"],"type":"string"}` | 形式 |
| `to` | — | `{"type":"string"}` | 書き出すファイル（人の操作のみ） |

## `keymap.get`

キーの割り当て

キーの割り当てを一覧します（プリセット・変更した行・衝突）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `changed` | — | `{"type":"boolean"}` | プリセットから変えた行だけ |
| `context` | — | `{"enum":["global","edit","arrange","roll","lane","automation","mixer","browser","text"],"type":"string"}` | この場所の行だけ |
| `key` | — | `{"type":"string"}` | キーで絞り込み |

## `keymap.import`

キーの割り当てを読み込み…

ほかのソフトの割り当てか keymap.toml を読み込みます（読み込んだキーは変更した行になります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `path` | — | `{"type":"string"}` | 読み込むファイル（Studio Pro は省略時に利用者の user.keyscheme） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `source` | 必須 | `{"enum":["studio-pro","toml"],"type":"string"}` | 読み込む形式 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `keymap.preset`

キーのプリセットを変更

キーのプリセットを替えます（既定では変更した行はそのまま残ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `keep` | — | `{"default":"mine","enum":["mine","none"],"type":"string"}` | 自分で変えた行の扱い（mine: 残す、none: 残さずプリセットのとおりにする） |
| `name` | 必須 | `{"enum":["studio-pro","cubase","logic","ableton","reaper"],"type":"string"}` | プリセット |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `keymap.reset`

キーをプリセットに戻す

キーの割り当てをプリセットの状態に戻します（キー・コマンド・場所で絞れます。省略時はすべて）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `command` | — | `{"type":"string"}` | このコマンドの行だけ |
| `context` | — | `{"enum":["global","edit","arrange","roll","lane","automation","mixer","browser","text"],"type":"string"}` | この場所だけ |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `key` | — | `{"type":"string"}` | このキーだけ |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `keymap.set`

キーを割り当て

キーにコマンドを割り当てます（ほかのコマンドと重なると衝突として表示されます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `command` | 必須 | `{"type":"string"}` | 実行するコマンド（引数付きでも可。例: "note.transpose --by 12"） |
| `context` | — | `{"enum":["global","edit","arrange","roll","lane","automation","mixer","browser","text"],"type":"string"}` | どこで効くか（省略時はコマンドの既定の場所か global） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | — | `{"type":"string"}` | このキーの代わりにします（コマンドの元のキー） |
| `key` | 必須 | `{"type":"string"}` | キー（例: Mod+Alt+V） |
| `replace` | — | `{"type":"boolean"}` | 同じキーのほかのコマンドからキーを外します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `shadow` | — | `{"type":"boolean"}` | 広い文脈のキーを意図して隠します |
| `swap` | — | `{"type":"boolean"}` | 同じキーのほかのコマンドに --from のキーを渡します |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `keymap.unset`

キーの割り当てを外す

キーの割り当てを外します（--command を付けるとそのコマンドからだけ）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `command` | — | `{"type":"string"}` | このコマンドからだけ外します |
| `context` | — | `{"enum":["global","edit","arrange","roll","lane","automation","mixer","browser","text"],"type":"string"}` | どこのキーか（省略時はすべての場所） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `key` | 必須 | `{"type":"string"}` | キー |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `keymap.which`

キーのコマンドを調べる

キーを押したときに実行されるコマンドを表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `command` | — | `{"type":"string"}` | キーを調べるコマンド |
| `context` | — | `{"enum":["global","edit","arrange","roll","lane","automation","mixer","browser","text"],"type":"string"}` | どこで押すか |
| `key` | — | `{"type":"string"}` | キーかコマンド名（例: Mod+Shift+Z、note.quantize） |

## `location.add`

場所を追加…

フォルダーを場所のタブとしてファイルのブラウザーに足します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `name` | — | `{"type":"string"}` | タブの名前（省略時はフォルダー名） |
| `path` | 必須 | `{"type":"string"}` | フォルダーのパス |

## `location.list`

場所の一覧

ファイルのブラウザーの場所のタブを一覧します（先頭は曲のフォルダー）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `location.remove`

場所を外す

場所のタブを外します（フォルダーとその中身はそのままです。曲のフォルダーは外せません）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `location` | 必須 | `{"type":"string"}` | 場所のタブの名前またはフォルダーのパス |

## `location.rename`

場所名を変更

場所のタブの名前を変えます（曲のフォルダーは変えられません）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `location` | 必須 | `{"type":"string"}` | 場所のタブの名前またはフォルダーのパス |
| `name` | 必須 | `{"type":"string"}` | 新しい名前 |

## `loop.clear`

ループ範囲を解除

ループ範囲を解除します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `loop`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `loop.duplicate`

ループを複製

ループ範囲の中身を、ループのすぐ後ろへ複製します（後ろにあったものは置き換わります）。ループ範囲は複製した側に移ります

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `loop`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `loop.end.set`

ループの終わりをここに

ループの終わりを指定位置にします（ループが無いときは頭から指定位置までのループを作ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `loop`。

キー: global: Alt+NumPad2 (`loop.end.set`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `loop.selection`

選択範囲をループ

選択範囲をループ

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `loop`。

キー: global: Shift+P (`loop.selection --no-snap`)、global: P (`loop.selection`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `no-snap` | — | `{"type":"boolean"}` | グリッドに合わせない |

## `loop.set`

ループ範囲を設定

ループ範囲を設定します（範囲内を繰り返し再生でき、書き出しの範囲にも使えます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `loop`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 終了位置（開始位置より後）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `start` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `loop.start.set`

ループの始まりをここに

ループの始まりを指定位置にします（ループが無いときは 1 小節のループを作ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `loop`。

キー: global: Alt+NumPad1 (`loop.start.set`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `macro.list`

マクロ

マクロを一覧します（名前と、順に行うコマンド、割り当てたキー）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `macro.remove`

マクロを削除

マクロを消します（割り当てたキーも外します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | マクロの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `macro.rename`

マクロ名を変更

マクロの名前を変えます（割り当てたキーはそのまま使えます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | いまの名前 |
| `new-name` | 必須 | `{"type":"string"}` | 新しい名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `macro.run`

マクロを実行

マクロの手順を順に行います。手順は 1 回の取り消しでまとめて元に戻せます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | マクロの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `macro.set`

マクロを保存

マクロを作ります（同じ名前があれば手順を置き換えます）。手順は曲を編集するコマンドの行で、--steps に [{"command": "note.quantize --grid 1/16"}, ...] の形で渡します。キーは keymap.set に "macro.run --name <名前>" を渡して割り当てます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | マクロの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `steps` | — | `{"items":{"type":"object"},"type":"array"}` | 順に行うコマンドの並び（{"command": "コマンドの行"}） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `marker.add`

マーカーを追加

指定位置にマーカーを追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: arrange: Insert / Y (`marker.add`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | — | `{"type":"string"}` | 名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `marker.add.named`

名前を付けてマーカーを追加…

再生位置にマーカーを追加する枠を開き、名前を入力させます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: arrange: Shift+Insert / Shift+Y (`marker.add.named`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `marker.color.set`

マーカーの色

マーカーの色を設定します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `color` | 必須 | `{"type":"string"}` | #rrggbb または none |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | マーカー（marker の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `marker.delete`

マーカーを削除

マーカーを削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | マーカー（marker の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `marker.loop`

次のマーカーまでをループに

次のマーカーまでをループ範囲にします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | マーカー（marker の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `marker.rename`

マーカー名を変更

マーカー名を変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | マーカー（marker の ID） |
| `name` | 必須 | `{"type":"string"}` | 名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `midi.get`

MIDI 設定を表示

オーディオ・MIDI の設定

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `midi.import`

MIDI ファイルをインポート…

MIDI ファイル（SMF の形式 0・1）をインポートし、トラックごと（形式 0 はチャンネルごと）に新しいインストゥルメントトラックへ置きます（1 回で取り消せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 置く位置（省略時は曲の頭）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | 必須 | `{"type":"string"}` | MIDI ファイル（絶対パスまたは曲のフォルダーからの相対パス） |
| `instrument` | — | `{"default":"Sampler","type":"string"}` | 新しいトラックのインストゥルメント |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tempo` | — | `{"default":"auto","enum":["auto","file","keep"],"type":"string"}` | ファイルのテンポと拍子の扱い（auto: 曲にクリップが無いときだけ曲のテンポと拍子にする、file: 曲のテンポと拍子にする、keep: 変えない） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `midi.import.place`

MIDI ファイルを配置

インポートした MIDI ファイルのパートを、新しいインストゥルメントトラックとして置きます（midi.import が使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 置く位置（省略時は曲の頭）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `instrument` | 必須 | `{"type":"string"}` | 新しいトラックのインストゥルメント |
| `meter` | — | `{"items":{"type":"object"},"type":"array"}` | 置く位置の小節からの拍子 [{bar, num, den}]（bar は 1 から。省略時は曲の拍子のまま） |
| `parts` | 必須 | `{"items":{"type":"object"},"type":"array"}` | パート [{name, clip, notes}] |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tempo` | — | `{"items":{"type":"object"},"type":"array"}` | 置く位置からのテンポ [{tick, bpm}]（省略時は曲のテンポのまま） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `midi.set`

MIDI 設定を変更

オーディオ・MIDI の設定

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `key` | 必須 | `{"type":"string"}` | 設定の名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `value` | 必須 | `{"type":"string"}` | 値 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `midimap.learn`

MIDI 学習

パートの値を、次に動かしたコントローラーのつまみに割り当てます（--cancel でやめる、--confirm で別のパートから譲り受ける）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `cancel` | — | `{"type":"boolean"}` | 待つのをやめる |
| `confirm` | — | `{"type":"boolean"}` | つまみを持っていたパートから譲り受ける |
| `target` | — | `{"type":"string"}` | 割り当てるもの（track.volume:<id>・track.pan:<id>） |

## `midimap.list`

コントローラーの割り当て

コントローラーの割り当てを一覧します（つまみやパッドと、動かすパートの値かコマンド）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `midimap.set`

コントローラーを割り当て

コントローラーのつまみ（CC）やパッド（ノート）を、パートの値かコマンドに割り当てます（同じつまみは置き換え、割り当てたメッセージは録音に入りません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `message` | 必須 | `{"type":"string"}` | メッセージ（cc:21 のつまみ、note:41 のパッド） |
| `port` | — | `{"type":"string"}` | MIDI の入力ポートの名前（省略時はどのポートでも） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | 必須 | `{"type":"string"}` | 動かすもの（track.volume:<トラックの id>・track.pan:<トラックの id>・コマンド名） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `midimap.unset`

コントローラーの割り当てを外す

コントローラーの割り当てを外します（--message を省くとすべて）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `message` | — | `{"type":"string"}` | メッセージ（cc:21 のつまみ、note:41 のパッド） |
| `port` | — | `{"type":"string"}` | MIDI の入力ポートの名前（省略時はどのポートでも） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | — | `{"type":"string"}` | 割り当て先（track.volume:<id>・track.pan:<id>・コマンド）で外す |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `mix.listen`

範囲の音を解析

指定した範囲をオフラインでレンダリングし、ピーク・RMS・ラウドネス（LUFS・トゥルーピーク・LRA）・ステレオ幅・小節ごとの値・スペクトルを JSON で返します（ファイルは書き出しません。同じ曲と範囲なら同じ値になります）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bands` | — | `{"default":"octave","enum":["octave","third"],"type":"string"}` | スペクトルの帯域（octave = オクターブ 10 帯域、third = 1/3 オクターブ 30 帯域） |
| `range` | — | `{"default":"song","type":"string"}` | 解析する範囲（song = 曲の先頭から最後のクリップの終わりまで、loop = ループ範囲、section:名前または id（カンマ区切りで複数）、3.1.0..5.1.0 のような位置の範囲） |
| `track` | — | `{"type":"string"}` | 解析するトラックまたはバス（省略時はマスター）（track の ID） |

## `mixer.bank`

次・前のバンク

ミキサーを、見えているチャンネルの数だけ右（--way next）か左（--way prev）へスクロールします

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: mixer: } (`mixer.bank --way next`)、mixer: { (`mixer.bank --way prev`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `way` | 必須 | `{"enum":["next","prev"],"type":"string"}` | スクロールの向き |

## `mixer.channel.next`

次のチャンネル

選んだチャンネルの次（右）のチャンネルを選びます。ミキサーが開いていればそのストリップを見せます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: mixer: ] (`mixer.channel.next`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `mixer.channel.prev`

前のチャンネル

選んだチャンネルの前（左）のチャンネルを選びます。ミキサーが開いていればそのストリップを見せます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: mixer: [ (`mixer.channel.prev`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `mixer.focus`

ミキサーでチャンネルを選択

下のドックをミキサーにして、チャンネルを選んで光らせます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `mixer`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | トラック（track の ID） |

## `mousemap.conflicts`

マウスの衝突

押している間のツールの切り替えが、同じ修飾キーのクリック・ドラッグを隠している箇所を一覧します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `mousemap.get`

マウスの割り当て

マウスの割り当てを一覧します（場所を省略すると変えた所と既定の所すべて）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `context` | — | `{"type":"string"}` | 場所（例: roll.select.note-body.drag、roll.select.hold） |

## `mousemap.reset`

マウスを既定に戻す

マウスの割り当てを既定に戻します（場所・修飾キーで絞れます。省略時はすべて）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `context` | — | `{"type":"string"}` | この場所だけ |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `modifier` | — | `{"type":"string"}` | この修飾キーだけ |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `mousemap.set`

マウスを割り当て

クリック・ドラッグに操作を、修飾キーを押している間にツールを割り当てます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `action` | 必須 | `{"type":"string"}` | 操作（例: note.duplicate）か、押している間のツール（例: tool:draw） |
| `context` | 必須 | `{"type":"string"}` | 場所（<area>.<tool>.<zone>.<click\|drag\|wheel> か <area>.<tool>.hold。area は roll / arrange / lane / automation / ruler） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `modifier` | 必須 | `{"type":"string"}` | 修飾キー（none / Shift / Mod / Mod+Shift / Alt / Mod+Alt、ドラッグ中は during.Shift など） |
| `move-from` | — | `{"type":"string"}` | ツールの切り替えを、この修飾キーから移す（移し先が使われていればエラーにして保存しない） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `mousemap.unset`

マウスの割り当てを外す

マウスの割り当てを外し、何もしないようにします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `context` | 必須 | `{"type":"string"}` | 場所 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `modifier` | 必須 | `{"type":"string"}` | 修飾キー |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.add`

ノートを追加

ノートを 1 つ追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 開始位置（曲の中の位置。小節.拍.tick または tick 数）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `clip` | 必須 | `{"type":"string"}` | 追加先の MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `length` | — | `{"default":"1/16","oneOf":[{"type":"integer"},{"type":"string"}]}` | 長さ（1/4・1/8t・tick 数）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `pitch` | 必須 | `{"maximum":127.0,"minimum":0.0,"type":"integer"}` | ピッチ（MIDI ノート番号 0〜127。60 が中央の C） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `velocity` | — | `{"default":100,"maximum":127.0,"minimum":1.0,"type":"integer"}` | ベロシティ 1〜127 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.delete`

ノートを削除

ノートを削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

キー: roll: Delete / Backspace (`note.delete`)、lane: Delete / Backspace (`note.delete`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.duplicate`

ノートを複製

ノートを複製します（--by tick 後ろ、--pitch_by 半音上にずらして配置）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

キー: roll: D (`note.duplicate`)、lane: D (`note.duplicate`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | 必須 | `{"type":"integer"}` | 移動量（tick。負の値で前へ） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `pitch_by` | — | `{"default":0,"maximum":127.0,"minimum":-127.0,"type":"integer"}` | 半音数（負の値で下へ） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.humanize`

ノートをヒューマナイズ

ノートのタイミングとベロシティに揺らぎを付けます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（note\|clip の ID のリスト） |
| `less` | — | `{"type":"boolean"}` | 変化量を半分にする |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `seed` | — | `{"default":1,"type":"integer"}` | 乱数の種 |
| `timing` | — | `{"default":20,"maximum":9600.0,"minimum":0.0,"type":"integer"}` | タイミングの最大変化量（tick） |
| `velocity` | — | `{"default":5,"maximum":126.0,"minimum":0.0,"type":"integer"}` | ベロシティの最大変化量 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.join`

ノートを結合

同じ高さのノートを結合します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

キー: roll: G (`note.join`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（note\|clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.list`

ノートの一覧

ノートを一覧表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | — | `{"type":"string"}` | このクリップのノートのみ（clip の ID） |
| `limit` | — | `{"default":500,"maximum":100000.0,"minimum":1.0,"type":"integer"}` | 最大件数 |

## `note.move`

ノートを移動

ノートを時間方向に移動します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | 必須 | `{"type":"integer"}` | 移動量（tick。負の値で前へ） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.mute`

ノートをミュート

ノートのミュートを切り替えます（--on / --off。省略時は現在の状態を反転）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `mute`。

キー: roll: Shift+M (`note.mute`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.put`

ノートをまとめて追加

複数のノートを 1 回の操作でまとめて追加します（鉛筆ツールでの連続入力に使用）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | 必須 | `{"type":"string"}` | 追加先の MIDI クリップ（clip の ID） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `grow` | — | `{"type":"boolean"}` | クリップの終わりより後ろのノートも置けるようにし、クリップをそのノートを含む小節の終わりまで伸ばします |
| `notes` | 必須 | `{"items":{"type":"object"},"type":"array"}` | [{at, pitch, length, velocity?, muted?}] の配列。at・length は「小節.拍.tick」または tick 数で、at はクリップの先頭から |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.quantize`

ノートをクオンタイズ

ノートの開始位置をクオンタイズします（--strength で強さを指定）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `quant`。

キー: roll: Q (`note.quantize`)、roll: Alt+Q (`note.quantize --strength 50`)、roll: Shift+Q (`note.quantize --restore`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | — | `{"maximum":100.0,"minimum":0.0,"type":"integer"}` | 終わりをクオンタイズする強さ %（省くと長さを保って動かします） |
| `freeze` | — | `{"type":"boolean"}` | 現在のタイミングを元のタイミングとして固定する |
| `grid` | — | `{"default":"1/16","oneOf":[{"type":"integer"},{"type":"string"}]}` | グリッド（1/16・1/8t・1/4d・tick 数）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |
| `groove` | — | `{"type":"string"}` | グルーヴにする MIDI クリップ（グリッドの代わりに、そのノートの開始とベロシティに揃えます）（clip の ID） |
| `groove-hits` | — | `{"items":{"type":"object"},"type":"array"}` | 外部グルーヴの [{at,velocity}] |
| `groove-period` | — | `{"maximum":9.223372036854776e+18,"minimum":1.0,"type":"integer"}` | 外部グルーヴの周期（tick） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `lengths` | — | `{"type":"boolean"}` | 長さもグリッドの倍数にクオンタイズする |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `range` | — | `{"maximum":100.0,"minimum":0.0,"type":"integer"}` | 線からこの割合（線の間の半分に対する %）以内のノートだけ動かす |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `restore` | — | `{"type":"boolean"}` | 元のタイミングに戻す |
| `strength` | — | `{"default":100,"maximum":100.0,"minimum":0.0,"type":"integer"}` | クオンタイズの強さ %（100 で完全にグリッドに揃える） |
| `swing` | — | `{"maximum":100.0,"minimum":0.0,"type":"integer"}` | スウィング %（1 つおきの線を、半ステップのこの割合だけ後ろへ） |
| `tuplet` | — | `{"maximum":7.0,"minimum":1.0,"type":"integer"}` | 連符（3・5・7。グリッドの 2 つ・4 つ分に n 個） |
| `velocity` | — | `{"maximum":100.0,"minimum":0.0,"type":"integer"}` | グルーヴのベロシティに揃える強さ % |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.resize`

ノートの長さを変更

ノートの長さを変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"integer"}` | 伸ばす量（tick。負の値で縮める） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `grid` | — | `{"default":"1/16","oneOf":[{"type":"integer"},{"type":"string"}]}` | グリッドの長さ（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `length` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 新しい長さ（1/8・tick 数など）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |
| `mode` | — | `{"enum":["next","grid","double","half"],"type":"string"}` | 長さの型 |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.set`

ノートをまとめて変更

複数のノートを 1 回の操作でまとめて変更します（ドラッグ操作の結果の確定に使用）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `notes` | 必須 | `{"items":{"type":"object"},"type":"array"}` | [{id, start?, length?, pitch?, velocity?}] の配列。start はクリップの先頭からの tick |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.split`

ノートを分割

ノートを指定位置で分割します（位置を含まないノートはそのまま）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `cut`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 分割する位置（曲の中の位置。小節.拍.tick または tick 数）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.split.grid`

ノートをグリッドで分割

ノートをグリッドの線ごとに分割します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

キー: roll: Mod+G (`note.split.grid`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `grid` | — | `{"default":"1/16","oneOf":[{"type":"integer"},{"type":"string"}]}` | グリッド（1/16・1/8t・1/4d・tick 数）（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（note\|clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.strum`

ストラム

同じ位置で始まる和音のノートを、低い音から（--spread が負なら高い音から）順にずらします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（note\|clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `spread` | 必須 | `{"maximum":9600.0,"minimum":-9600.0,"type":"integer"}` | 最初のノートと最後のノートの開始の差（tick。負で高い音から） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.transpose`

ノートをトランスポーズ

ノートを半音単位でトランスポーズします（例: --by 12 で 1 オクターブ上）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

キー: roll: Alt+Up / Up (`note.transpose --by 1`)、roll: Alt+Shift+Up / Shift+Up (`note.transpose --by 12`)、roll: Alt+Down / Down (`note.transpose --by -1`)、roll: Alt+Shift+Down / Shift+Down (`note.transpose --by -12`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | 必須 | `{"maximum":127.0,"minimum":-127.0,"type":"integer"}` | 半音数（負の値で下へ） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `overlap` | — | `{"enum":["allow","trim"],"type":"string"}` | 同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.value.set`

ノートの値を設定…

選択したノートの値を設定します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `field` | 必須 | `{"enum":["start","length","pitch","velocity"],"type":"string"}` | 変更する値 |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（note の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `value` | 必須 | `{"type":"integer"}` | 値 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.velocity.humanize`

ベロシティを散らす

ノートのベロシティだけを少し散らします（タイミングは動かしません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `amount` | — | `{"default":8,"maximum":126.0,"minimum":0.0,"type":"integer"}` | ベロシティの最大変化量 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（note\|clip の ID のリスト） |
| `less` | — | `{"type":"boolean"}` | 変化量を半分にする |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `seed` | — | `{"default":1,"type":"integer"}` | 乱数の種 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.velocity.set`

ベロシティを設定

ノートのベロシティを設定します（--value で同じ値に、--by で増減。1〜127 の範囲に収めます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"integer"}` | 増減する量 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（クリップの ID を指定すると、そのクリップの全ノート）（note\|clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `value` | — | `{"maximum":127.0,"minimum":1.0,"type":"integer"}` | ベロシティ 1〜127 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.velocity.shape`

ベロシティに強弱を付ける

ノートのベロシティに強弱を付けます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `amount` | — | `{"default":10,"maximum":126.0,"minimum":0.0,"type":"integer"}` | 変化量 |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（note\|clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `shape` | 必須 | `{"enum":["stronger","softer","equal","crescendo","decrescendo"],"type":"string"}` | 強弱の型 |
| `value` | — | `{"default":100,"maximum":127.0,"minimum":1.0,"type":"integer"}` | 揃えるベロシティ |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `note.velocity.transform`

ベロシティを変形

ベロシティを、平均を中心に縮尺し（--scale）、時間に沿って傾けます（--tilt）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `roll`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ノート（note\|clip の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `scale` | — | `{"default":100,"maximum":400.0,"minimum":0.0,"type":"integer"}` | 縮尺 %（100 で変えない。0 で平均に揃える） |
| `tilt` | — | `{"default":0,"maximum":126.0,"minimum":-126.0,"type":"integer"}` | 最後のノートで足す量（最初のノートは 0。負で下げる） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `ops.get-result`

操作の結果

操作の結果をページごとに取得します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | 結果 ID |
| `page` | — | `{"default":0,"maximum":1000000.0,"minimum":0.0,"type":"integer"}` | ページ（0 から） |

## `ops.hello`

接続

コマンドのプロトコルを照合します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `version` | 必須 | `{"type":"integer"}` | プロトコルの版番号 |

## `ops.list`

コマンドの一覧

使用できるコマンドを一覧表示します（--grep で名前と説明を絞り込み）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `grep` | — | `{"type":"string"}` | この語句を含むものだけ |

## `ops.receipt`

操作の状態

操作の受領証を取得します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | 受領証 ID |

## `ops.schema`

コマンドの引数

コマンドの引数の JSON Schema を表示します（MCP の tools/list と同じ形式）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `name` | — | `{"type":"string"}` | コマンド名（省略時はすべて） |

## `plugin.activate`

プラグインを有効に戻す

無効にしたプラグインを有効に戻します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `power`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | プラグイン（plugin の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.add`

プラグインを追加

インサートにプラグインを追加します（インストゥルメントは先頭に1つだけ）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `format` | — | `{"default":"builtin","type":"string"}` | vst3・clap・builtin など |
| `index` | — | `{"type":"integer"}` | インサート内の位置（省略時は末尾） |
| `kind` | — | `{"default":"effect","enum":["effect","instrument"],"type":"string"}` | エフェクトかインストゥルメントか |
| `name` | 必須 | `{"type":"string"}` | プラグインの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `uid` | — | `{"type":"string"}` | 形式ごとの固有 ID（省略時は名前） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.builtin.list`

内蔵プラグインの一覧

内蔵のインストゥルメントとエフェクトを、パラメーターの規格つきで一覧表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `plugin.bypass`

プラグインをバイパス

プラグインのバイパスを切り替えます（--on / --off。省略時は現在の状態を反転）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | プラグイン（plugin の ID のリスト） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.collect`

プラグインの値を反映

プラグインの画面で変更した値を曲に反映します（1 回で取り消せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.copy_to`

プラグインをコピー

インサートを別のチャンネルにコピーします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `index` | — | `{"maximum":9.223372036854776e+18,"minimum":0.0,"type":"integer"}` | 挿入する位置（省略時は末尾） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | — | `{"type":"string"}` | 移動先のチャンネル（省略時は現在のチャンネル）（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.deactivate`

プラグインを無効にする

プラグインを無効にします（読み込みを止めて CPU を空けます。状態は残り、バイパスとは別です）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `power`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | プラグイン（plugin の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.duplicate`

プラグインを複製

インサートを複製します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `index` | — | `{"maximum":9.223372036854776e+18,"minimum":0.0,"type":"integer"}` | 挿入する位置（省略時は末尾） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | — | `{"type":"string"}` | 移動先のチャンネル（省略時は現在のチャンネル）（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.editor.open`

エフェクトの画面を開く

エフェクトの画面を開きます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: F11 (`plugin.editor.open`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |

## `plugin.insert`

最近使ったエフェクトを追加

最近使ったエフェクトをチャンネルの末尾に挿します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `item` | 必須 | `{"type":"string"}` | エフェクトの名前（内蔵は builtin:<id>） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.isolation.set`

プラグインの実行方法を設定

プラグインを実行するプロセスを設定します（inprocess: エンジン内 / shared: 共有の別プロセス / dedicated: 専用の別プロセス）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | プラグイン（plugin の ID のリスト） |
| `mode` | 必須 | `{"enum":["inprocess","shared","dedicated"],"type":"string"}` | 実行するプロセス |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.move`

プラグインを移動

インサートを移動します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `index` | — | `{"maximum":9.223372036854776e+18,"minimum":0.0,"type":"integer"}` | 挿入する位置（省略時は末尾） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | — | `{"type":"string"}` | 移動先のチャンネル（省略時は現在のチャンネル）（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.param.list`

パラメーターの一覧

プラグインのパラメーターを一覧表示します（内蔵プラグインは範囲・単位・既定・今の値つき）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |

## `plugin.param.set`

パラメーターを設定

プラグインのパラメーターを設定します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `param` | 必須 | `{"type":"string"}` | パラメーターの名前 |
| `plain` | — | `{"type":"string"}` | 内蔵プラグインの値を単位で指定します（-6 dB なら -6。0〜1 の割合は --value を使います。選択肢は名前、オン/オフは on/off でも可） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `value` | — | `{"type":"number"}` | 値（内蔵プラグインと VST3 は 0〜1。--value か --plain のどちらか） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.pick`

エフェクトを追加…

チャンネルの ＋ の下に、挿すエフェクトの一覧を開きます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | トラック（track の ID） |

## `plugin.preset.apply`

プリセットを適用

プリセットの状態を適用します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `state` | — | `{"type":"string"}` | 保存された状態 |
| `values` | 必須 | `{"items":{"type":"object"},"type":"array"}` | パラメーターの値 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.preset.choose`

プリセットを選ぶ

内蔵エフェクトのプリセットを読み込みます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `preset` | 必須 | `{"type":"string"}` | プリセットの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.preset.file`

プリセットをファイルで読み書き

プリセットのファイルを選ぶ窓を開きます（--save で書き出し、省略時は読み込み）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `save` | — | `{"type":"boolean"}` | ファイルへ書き出す |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.preset.load`

プリセットを読み込む

プリセットの値をまとめてプラグインに設定します（1 回の操作として元に戻せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `preset` | 必須 | `{"type":"string"}` | プリセットの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `values` | 必須 | `{"items":{"type":"object"},"type":"array"}` | 値の一覧（{param, value} の配列） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.preset.next`

次のプリセット

いちばん手前のプラグインの窓に、次のプリセットを読み込みます（最後の次は最初）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

キー: global: Alt+PageDown (`plugin.preset.next`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.preset.open`

プリセットを読み込む…

プリセットをファイルから読み込みます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | 必須 | `{"type":"string"}` | ファイル |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.preset.prev`

前のプリセット

いちばん手前のプラグインの窓に、前のプリセットを読み込みます（最初の前は最後）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

キー: global: Alt+PageUp (`plugin.preset.prev`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.preset.save`

プリセットを保存…

プラグインのプリセットをファイルに保存します

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `file` | 必須 | `{"type":"string"}` | ファイル |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.reload`

プラグインを再読み込み

プラグインを再読み込みします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `sync`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.remove`

プラグインを削除

プラグインを削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | プラグイン（plugin の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.replace`

プラグインを差し替え

プラグインを別のものに差し替えます（位置はそのまま）。--slot instrument では、インストゥルメントトラックのインストゥルメントを差し替えます（インストゥルメントが無ければ先頭に入れます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `format` | — | `{"default":"builtin","type":"string"}` | vst3・clap・builtin など |
| `id` | — | `{"type":"string"}` | 差し替えるプラグイン（plugin の ID） |
| `name` | 必須 | `{"type":"string"}` | 新しいプラグインの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `slot` | — | `{"default":"insert","enum":["insert","instrument"],"type":"string"}` | 差し替える場所（insert は --id のプラグイン、instrument はトラックのインストゥルメント） |
| `track` | — | `{"type":"string"}` | トラック（--slot instrument のとき）（track の ID） |
| `uid` | — | `{"type":"string"}` | 形式ごとの固有 ID（省略時は名前） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.rescan`

プラグインを再スキャン

インストール済みのプラグインを再スキャンします（新規と変更されたもののみ。--full ですべて）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `background` | — | `{"type":"boolean"}` | バックグラウンドでスキャンし、すぐにジョブの id を返す（進行状況は export.status） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `full` | — | `{"type":"boolean"}` | キャッシュを使わず、すべてのプラグインをスキャンし直す |
| `path` | — | `{"type":"string"}` | このプラグインのファイルは、前回の結果に関わらず開き直す（使えなかったものを試し直すとき） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.restart`

プラグインを再起動

クラッシュしたプラグインのプロセスを再起動します（クラッシュ前の状態に復元します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `sync`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | — | `{"type":"string"}` | 再起動するプラグイン（省略時はクラッシュしたプロセスすべて）（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.scan.status`

スキャンの状況

プラグインのスキャンの進行状況と、見つかったプラグイン・読み込めないプラグインの数を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `plugin.sidechain.add`

サイドチェーンの送り元を追加

プラグインのサイドチェーンに送り元を追加します（既定はポストフェーダーのセンド。--output は送り元の出力そのもので、量は掛かりません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"type":"string"}` | 送り元のチャンネル（track の ID） |
| `id` | 必須 | `{"type":"string"}` | キーを受けるプラグイン（plugin の ID） |
| `level` | — | `{"maximum":6.0,"minimum":-60.0,"type":"number"}` | 送る量（dB、-60〜6。--output では使えません） |
| `output` | — | `{"type":"boolean"}` | 送り元の出力そのものを送る |
| `pre` | — | `{"type":"boolean"}` | プリフェーダーで送る |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.sidechain.enable`

サイドチェーンを切り替え

サイドチェーンの入・切（Int / Ext）をします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | キーを受けるプラグイン（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `state` | 必須 | `{"enum":["on","off"],"type":"string"}` | 入・切 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.sidechain.list`

サイドチェーンの送り元の一覧

サイドチェーンの送り元を一覧します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | キーを受けるプラグイン（plugin の ID） |

## `plugin.sidechain.remove`

サイドチェーンの送り元を削除

サイドチェーンの送り元を外します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"type":"string"}` | 送り元のチャンネル（track の ID） |
| `id` | 必須 | `{"type":"string"}` | キーを受けるプラグイン（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.sidechain.set`

サイドチェーンの送り元を設定

サイドチェーンの送り元の量やプリ／ポストを変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"type":"string"}` | 送り元のチャンネル（track の ID） |
| `id` | 必須 | `{"type":"string"}` | キーを受けるプラグイン（plugin の ID） |
| `level` | — | `{"maximum":6.0,"minimum":-60.0,"type":"number"}` | 送る量（dB） |
| `position` | — | `{"enum":["pre","post"],"type":"string"}` | プリフェーダーかポストフェーダーか |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.state.set`

プラグインの状態を置き換え

プラグインの保存状態を置き換えます（プラグインが出力したデータを base64 で指定。空にすると削除）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | プラグイン（plugin の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `state` | 必須 | `{"type":"string"}` | 状態（base64。空の文字列で削除） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `plugin.stats`

プラグインの処理時間

プラグインごとの処理時間を表示します（直近 1 秒ほどの平均と最大、再生の時間に対する割合。重いものから順。別プロセスのプラグインは、そのプロセスで測った時間）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `gauge`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `track` | — | `{"type":"string"}` | このトラックのプラグインだけ表示する（track の ID） |

## `plugin.status`

プラグインの状態

別のプロセスで実行中のプラグインの状態を表示します（クラッシュの有無・遅延・バイパスしたブロック数）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `prefs.get`

設定を表示

設定の値を表示します（名前か、edit. のような先頭の部分。省略時はすべて）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `gear`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `key` | — | `{"type":"string"}` | 設定の名前か、その先頭の部分（例: edit.automation） |

## `prefs.reset`

設定を既定に戻す

設定を既定の値に戻します（名前か先頭の部分。省略時はすべて）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `key` | — | `{"type":"string"}` | 設定の名前か、その先頭の部分 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `prefs.set`

設定を変更

設定の値を変えます（すぐに保存し、開いている画面にも反映されます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `add` | — | `{"type":"boolean"}` | 一覧の設定（フォルダー）に value を追加します |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `key` | 必須 | `{"type":"string"}` | 設定の名前（例: edit.automation.follows-clips） |
| `remove` | — | `{"type":"boolean"}` | 一覧の設定（フォルダー）から value を外します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `value` | — | `{"type":"string"}` | 値（true / false、選択肢の名前、数、フォルダーなど） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `preset.factory.restore`

工場出荷のプリセットを読み込み直す

一覧に無い内蔵のプリセットを足し直します（あるものは上書きしません）

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `device` | — | `{"type":"string"}` | 機器の uid か名前（その機器のプリセットだけに絞ります） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `preset.list`

プリセットの一覧

機器のプリセットを一覧します（機器ごとに Default が先頭、ほかは名前の順）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `device` | — | `{"type":"string"}` | 機器の uid か名前（その機器のプリセットだけに絞ります） |
| `kind` | — | `{"enum":["fx","instrument"],"type":"string"}` | プリセットの種類 |
| `query` | — | `{"type":"string"}` | 名前で絞る文字列 |

## `preset.load`

プリセットを読み込み

プリセットをトラックの機器に読み込みます（1 回の取り消しで戻せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `device` | — | `{"type":"string"}` | 機器の uid か名前（その機器のプリセットだけに絞ります） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `of` | — | `{"type":"string"}` | プリセットを読み込むプラグイン（plugin の ID） |
| `preset` | 必須 | `{"type":"string"}` | プリセットの名前か、.xpreset / plugin.preset のファイル |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `preset.put`

プリセットを機器に反映

読み込んだプリセットの内容を、トラックの機器に反映します（preset.load から使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `lost` | — | `{"type":"string"}` | 見つからないため外したサンプルの名前（JSON の配列。調整の行に出します） |
| `missing` | — | `{"type":"string"}` | この PC に無いプラグインの uid（コンマ区切り。空のスロットとして残します） |
| `name` | — | `{"type":"string"}` | プリセットの名前（プラグインに記録します） |
| `of` | — | `{"type":"string"}` | プリセットを読み込むプラグイン（plugin の ID） |
| `preset` | 必須 | `{"type":"string"}` | プリセットの内容（JSON） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `preset.remove`

プリセットを削除

プリセットを削除します（「Default」は消せません）

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `device` | — | `{"type":"string"}` | 機器の uid か名前（その機器のプリセットだけに絞ります） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `preset` | 必須 | `{"type":"string"}` | プリセットの名前か、.xpreset / plugin.preset のファイル |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `preset.rename`

プリセット名を変更

プリセットの名前を変えます

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `device` | — | `{"type":"string"}` | 機器の uid か名前（その機器のプリセットだけに絞ります） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | 新しい名前 |
| `preset` | 必須 | `{"type":"string"}` | プリセットの名前か、.xpreset / plugin.preset のファイル |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `preset.save`

プリセットを保存

機器の今の設定を .xpreset のプリセットに保存します（同じ名前があるときは --overwrite で上書きします）

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `kind` | 必須 | `{"enum":["fx","instrument"],"type":"string"}` | プリセットの種類 |
| `name` | 必須 | `{"type":"string"}` | プリセットの名前 |
| `of` | — | `{"type":"string"}` | プリセットにするプラグイン（plugin の ID） |
| `overwrite` | — | `{"type":"boolean"}` | この機器に同じ名前（大文字と小文字の違いは同じ）のプリセットがあれば上書きします |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `project.cleanup`

使われていない録音を整理

どのクリップにも履歴にもバージョンにも使われていない録音のファイルを、ごみ箱に移動します（dry_run で一覧だけを返します）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `project.get`

曲の概要

曲の概要（名前・テンポ・拍子・セクション・各要素の数）を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `project.new`

新しい曲

新しい曲

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

キー: global: Mod+N (`project.new`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `project.open`

曲を開く…

別の曲を開きます。DAWproject（.dawproject）は新しい曲として開き、読み込めなかったものを一覧にして返します

事前条件: ユーザーのみ。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `open`。

キー: global: Mod+O (`project.open`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `media` | — | `{"type":"string"}` | .dawproject の中の音のファイルを取り出すフォルダー（省略時は一時フォルダー。保存すると曲のフォルダーにコピーします） |
| `path` | 必須 | `{"type":"string"}` | 曲のフォルダー、または .dawproject のファイル |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `project.rename`

曲名を変更

曲の名前を変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | 新しい名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `project.save`

保存

曲を保存します

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `saved`。

キー: global: Mod+S (`project.save`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `project.save-as`

名前を付けて保存…

名前を付けて保存します（履歴とバージョンもコピーされます）

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `file`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `path` | 必須 | `{"type":"string"}` | 保存先のフォルダー |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `project.settings`

曲の設定

曲の設定（サンプルレート・ビット深度・ファイルの置き場所）を変更します。サンプルレートとビット深度は再生と書き出しの初期値になります。置き場所を変えても、今あるファイルは動かしません

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: global: Mod+. (`project.settings`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bit_depth` | — | `{"type":"integer"}` | ビット深度（16・24・32。32 は浮動小数点） |
| `copy_imports` | — | `{"type":"boolean"}` | 曲のフォルダーの外からインポートしたファイルを、置き場所の Imports にコピーします（false でコピーしません。既定は true） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `media_folder` | — | `{"type":"string"}` | 録音・バウンス・フリーズ・コピーしたファイルの置き場所（曲のフォルダーからの相対パスか絶対パス。既定は Media） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `sample_rate` | — | `{"maximum":384000.0,"minimum":8000.0,"type":"integer"}` | サンプルレート（Hz。8000〜384000） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `quantize.program`

クオンタイズのプログラムを選択

クオンタイズのプログラム A〜G を選びます（--save で今の値をそのプログラムに保存します）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+Alt+1 (`quantize.program --program A`)、global: Mod+Alt+2 (`quantize.program --program B`)、global: Mod+Alt+3 (`quantize.program --program C`)、global: Mod+Alt+4 (`quantize.program --program D`)、global: Mod+Alt+5 (`quantize.program --program E`)、global: Mod+Alt+6 (`quantize.program --program F`)、global: Mod+Alt+7 (`quantize.program --program G`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `program` | 必須 | `{"enum":["A","B","C","D","E","F","G"],"type":"string"}` | プログラム |
| `save` | — | `{"type":"boolean"}` | 選ぶ代わりに、今のグリッドとクオンタイズの設定を保存する |

## `rack.show`

ラックを表示

トラックのラック（音源と FX の木）を JSON で返します。ラックの無いトラックは直列の形を implicit: true で返します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `track` | 必須 | `{"type":"string"}` | トラック（track の ID） |

## `range.bounce`

選択範囲をバウンス

範囲のトラックの音を、インサートとボリューム・パンを通してミックスし、下の新しいトラックに置きます（センドとマスターは通しません）。範囲に収まる元のクリップはミュートします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `per-track` | — | `{"type":"boolean"}` | トラックごとに新しいトラックを作る（ボリューム・パンは焼かない） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tail` | — | `{"default":10.0,"maximum":10.0,"minimum":0.0,"type":"number"}` | 残響の尾を含める最大の秒数 |
| `to` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `tracks` | 必須 | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `range.copy`

範囲の中身をコピー

範囲の中身をクリップボードへコピーします（両端で切った形で入れ、曲は変えません）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `end` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `loop` | — | `{"type":"boolean"}` | ループ範囲を使う（--range loop と同じ） |
| `range` | — | `{"type":"string"}` | loop・開始..終了・section:名前（start・end・loop の代わり） |
| `start` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |

## `range.cut`

範囲の中身を切り取り

範囲の中身を切り取ってクリップボードへ置きます（両端でクリップを分割し、範囲の中のクリップを消す。--ripple で範囲のあったトラックの後ろを詰める）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `automation` | — | `{"enum":["follow","stay"],"type":"string"}` | リップル編集で動くクリップの範囲のオートメーション: follow で一緒に動かす（既定）、stay でその場に残す |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `loop` | — | `{"type":"boolean"}` | ループ範囲を使う（--range loop と同じ） |
| `range` | — | `{"type":"string"}` | loop・開始..終了・section:名前（start・end・loop の代わり） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `ripple` | — | `{"type":"boolean"}` | リップル編集: 同じトラックの後ろのクリップを詰める・押す（省略時は設定の edit.ripple） |
| `start` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `range.delete`

範囲の中身を削除

範囲の中身を削除します（両端でクリップを分割し、範囲の中のクリップを消す。--ripple で範囲のあったトラックの後ろを詰める）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `automation` | — | `{"enum":["follow","stay"],"type":"string"}` | リップル編集で動くクリップの範囲のオートメーション: follow で一緒に動かす（既定）、stay でその場に残す |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `loop` | — | `{"type":"boolean"}` | ループ範囲を使う（--range loop と同じ） |
| `range` | — | `{"type":"string"}` | loop・開始..終了・section:名前（start・end・loop の代わり） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `ripple` | — | `{"type":"boolean"}` | リップル編集: 同じトラックの後ろのクリップを詰める・押す（省略時は設定の edit.ripple） |
| `start` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `range.duplicate`

範囲の中身を複製

範囲の中身を、範囲の直後に複製します（両端でクリップを分割し、範囲の長さだけ後ろへ複製する）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | クリップ（clip の ID のリスト） |
| `loop` | — | `{"type":"boolean"}` | ループ範囲を使う（--range loop と同じ） |
| `range` | — | `{"type":"string"}` | loop・開始..終了・section:名前（start・end・loop の代わり） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `start` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `range.from-clips`

クリップから範囲を作成

選択中のクリップがある時間を範囲にします

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: arrange: Alt+R / Shift+R (`range.from-clips`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `range.shape`

範囲を直線・曲線・平らに

オートメーションレーン（またはテンポの行）の範囲を、直線・曲線・平らにします（範囲の中のポイントは両端の 2 つになり、外の線は変わりません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `curve` | — | `{"maximum":1.0,"minimum":-1.0,"type":"number"}` | 曲線の曲がり（-1〜1。省略時は 0.5。テンポには使えません） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `shape` | 必須 | `{"enum":["line","curve","flat"],"type":"string"}` | 形 |
| `target` | — | `{"type":"string"}` | 対象（volume・pan・mute・send:<ID>・param:<ID>:<名前>） |
| `tempo` | — | `{"type":"boolean"}` | テンポの行を対象にする |
| `to` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `track` | — | `{"type":"string"}` | オートメーションのトラック（テンポのときは不要）（track の ID） |
| `value` | — | `{"type":"number"}` | 平らにする値（省略時は範囲の頭の値） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `range.time.delete`

時間を削除

範囲の時間を取り除きます。範囲の中のクリップは消え、曲全体（全トラックのクリップ・オートメーション・テンポ・拍子・セクション・マーカー・ループ）が前へつまります

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

キー: arrange: Mod+Alt+D (`range.time.delete`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bars` | — | `{"type":"number"}` | 範囲の長さ（小節） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了（--length の代わり）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `length` | — | `{"type":"integer"}` | 範囲の長さ（tick） |
| `range` | — | `{"type":"string"}` | loop・開始..終了・section:名前（start・end・length の代わり） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `start` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `range.time.insert`

時間を挿入

範囲の位置に空の時間を挿入します。曲全体（全トラックのクリップ・オートメーション・テンポ・拍子・セクション・マーカー・ループ）が後ろへずれ、範囲の頭にかかるクリップは分かれます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: arrange: Mod+Alt+I (`range.time.insert`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bars` | — | `{"type":"number"}` | 範囲の長さ（小節） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了（--length の代わり）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `length` | — | `{"type":"integer"}` | 範囲の長さ（tick） |
| `range` | — | `{"type":"string"}` | loop・開始..終了・section:名前（start・end・length の代わり） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `start` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `range.transform`

範囲の値を変形

オートメーションレーン（またはテンポの行）の範囲の値を、縮尺と傾きで変形します（外の線は変わりません）。画面で scale と tilt を両方省くと、範囲に変形の枠を出します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: arrange: Alt+T (`range.transform`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `scale` | — | `{"type":"number"}` | 範囲の中の最小と最大の中央を軸にした縮尺（省略時は 1） |
| `target` | — | `{"type":"string"}` | 対象（volume・pan・mute・send:<ID>・param:<ID>:<名前>） |
| `tempo` | — | `{"type":"boolean"}` | テンポの行を対象にする |
| `tilt` | — | `{"type":"number"}` | 範囲の終わりでの上げ下げ量（頭は 0。オートメーションはレーンの高さに対する割合、テンポは bpm。省略時は 0） |
| `to` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `track` | — | `{"type":"string"}` | オートメーションのトラック（テンポのときは不要）（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `range.trim`

範囲の値を上げ下げ

オートメーションレーン（またはテンポの行）の範囲の値を、まとめて上げ下げします（外の線は変わりません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `delta` | 必須 | `{"type":"number"}` | 上げ下げする量（オートメーションはレーンの高さに対する割合 -1〜1、テンポは bpm） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `target` | — | `{"type":"string"}` | 対象（volume・pan・mute・send:<ID>・param:<ID>:<名前>） |
| `tempo` | — | `{"type":"boolean"}` | テンポの行を対象にする |
| `to` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `track` | — | `{"type":"string"}` | オートメーションのトラック（テンポのときは不要）（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `record.count-in`

プリカウント

プリカウントを切り替えます（録音の前にクリックを数えます。audio.count-in と同じ値です）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `record`。

キー: global: Shift+C (`record.count-in`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bars` | — | `{"maximum":8.0,"minimum":1.0,"type":"integer"}` | プリカウントの小節数（1〜8） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `record.input-quantize`

入力クオンタイズ

入力クオンタイズを切り替えます（録音するノートの頭を値にそろえます。長さは変えません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `record`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `value` | — | `{"enum":["1/4","1/8","1/16","1/8T"],"type":"string"}` | そろえる値 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `record.postroll`

ポストロール

ポストロールを切り替えます（パンチ範囲の後も再生を続けて止めます。オートパンチが入のときだけ使えます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `record`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bars` | — | `{"maximum":8.0,"minimum":1.0,"type":"integer"}` | ポストロールの小節数（1〜8） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `record.preroll`

プリロール

プリロールを切り替えます（パンチ範囲の手前から再生を始めます。省略時は現在の状態を切り替え）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `record`。

キー: global: O (`record.preroll`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bars` | — | `{"maximum":8.0,"minimum":1.0,"type":"integer"}` | プリロールの小節数（1〜8） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `record.punch`

オートパンチ

オートパンチを切り替えます（ループの範囲の中だけを録音します。省略時は現在の状態を切り替え）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `record`。

キー: global: I (`record.punch`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `record.start`

録音

録音準備のトラックの入力を録音します（停止中なら再生も開始します）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `record`。

キー: global: NumPad* / MediaRecord (`record.start`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 録音を開始する位置（省略時は現在の位置）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `latency` | — | `{"type":"number"}` | 入力のレイテンシー（ms。出力から入力に戻るまでの時間。省略時はデバイスの値） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `record.status`

録音の状態

録音の状態（録音中のトラック・長さ・欠落したフレーム数、パンチ・プリロール・ポストロール・プリカウント・入力クオンタイズの設定）を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `record`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `record.stop`

録音を停止

録音を停止し、録音したテイクをクリップにします（再生も停止します。1 回の録音は 1 回で取り消せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `record`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `record.take.add`

テイクを配置

録音したテイクをクリップとして配置します（1 回で取り消せます。既存のクリップは変更しません。同じトラックのテイクは最後のものだけが再生されます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `record`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `takes` | 必須 | `{"items":{"type":"object"},"type":"array"}` | [{track, start, length, file?, offset?, notes?: [{start, length, pitch, velocity?}], name?, muted?}] の配列。start・length は tick、file は曲のフォルダーからの相対パス（オーディオトラック）、offset はファイルの中で使い始める位置（tick）、notes の start はクリップの先頭から（インストゥルメントトラック） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `roll.drum.hide`

ドラムの行を隠す

ドラム表示の行を隠します（--show で戻します）。ノートのある行は隠しても出たままです。トラックごとに覚えます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `pitch` | 必須 | `{"maximum":127.0,"minimum":0.0,"type":"integer"}` | 行のノート番号 |
| `show` | — | `{"type":"boolean"}` | 隠した行を戻す |

## `roll.drum.move`

ドラムの行を移動

ドラム表示の行を上下へ移します（--by -1 で 1 つ上）。トラックごとに覚えます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | 必須 | `{"type":"integer"}` | 移す行の数（負で上へ） |
| `clip` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `pitch` | 必須 | `{"maximum":127.0,"minimum":0.0,"type":"integer"}` | 行のノート番号 |

## `roll.drum.rename`

ドラムの行名を変更

ドラム表示の行の名前を変えます（空で元の名前に戻します）。トラックごとに覚えます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `name` | 必須 | `{"type":"string"}` | 名前 |
| `pitch` | 必須 | `{"maximum":127.0,"minimum":0.0,"type":"integer"}` | 行のノート番号 |

## `roll.drum.reset`

ドラムの行を元に戻す

ドラム表示の行の並び・隠した行・名前を、マップのとおりに戻します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |

## `roll.view`

ピアノロールの見方（鍵盤・ドラム）

パートをピアノロールで鍵盤で見るか、ドラムの行（ドラムマップ）で見るかを選びます。パートごと、マップはトラックごとに覚えます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | 必須 | `{"type":"string"}` | クリップ（clip の ID） |
| `map` | — | `{"enum":["gm","kit"],"type":"string"}` | ドラムマップ |
| `mode` | — | `{"enum":["keys","drums"],"type":"string"}` | 見方 |

## `section.add`

セクションを追加

セクション（A メロ・サビなど）を追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 開始位置（--range のときは不要）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `bars` | — | `{"type":"number"}` | 長さ（小節。省略時は 1 小節で、次のセクションの手前までに収まります） |
| `color` | — | `{"default":"none","type":"string"}` | #rrggbb |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `length` | — | `{"type":"integer"}` | 長さ（tick） |
| `name` | 必須 | `{"type":"string"}` | 名前 |
| `range` | — | `{"type":"string"}` | loop または 開始..終了（at・length の代わり） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.color.set`

セクションの色を設定

セクションの色を設定します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `color` | 必須 | `{"type":"string"}` | #rrggbb または none |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.copy`

セクションをコピー

セクションを、中のクリップ・オートメーション・テンポごとクリップボードにコピーします

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |

## `section.cut`

セクションを切り取り

セクションをクリップボードに置き、その時間を曲から取り除きます（後ろが前へつまります）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.delete`

セクションを削除

セクションを削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.duplicate`

セクションを複製

セクションを複製します。既定では元のすぐ後ろに差し込み、後ろのセクションは複製の長さだけ後ろへずれます（--at end で最後のセクションの後ろ）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: arrange: Alt+D (`section.duplicate`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"default":"after","type":"string"}` | after（既定）か end |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `index` | — | `{"maximum":1000000.0,"minimum":0.0,"type":"integer"}` | 複製を置く順番（0 が先頭。--at の代わり。複製の後の順番） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.move`

セクションを移動

セクションを移動します（--at で位置を指定、--by で相対移動）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 新しい開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `by` | — | `{"type":"integer"}` | 移動量（tick） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.paste`

セクションを貼り付け

クリップボードのセクションを貼り付けます。その位置に時間を差し込み、後ろは貼り付けた長さだけ後ろへずれます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 貼り付ける位置（省略時は最後のセクションの後ろ）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.rename`

セクション名を変更

セクションの名前を変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `name` | 必須 | `{"type":"string"}` | 新しい名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.reorder`

セクションを並べ替え

セクションを並べ替えます。全トラックのクリップとオートメーションのポイント、マーカー、セクション内のテンポと拍子の変化も一緒に移動します（各セクションは次のセクションまでの空白ごと移動します）。境にまたがるクリップは境で分けます。ループ範囲は動きません

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `index` | 必須 | `{"maximum":1000000.0,"minimum":0.0,"type":"integer"}` | 新しい順番（0 が先頭） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.resize`

セクションの長さを変更

セクションの長さを変更します（次のセクションの手前までで止まり、重なりません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bars` | — | `{"type":"number"}` | 長さ（小節） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `length` | — | `{"type":"integer"}` | 長さ（tick） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `section.split`

セクションを分割

セクションを指定の位置で 2 つに分けます（名前と色は引き継ぎます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 分ける位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `select.add`

選択に追加

選択に追加します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `ids` | 必須 | `{"items":{"type":"string"},"type":"array"}` | 選択する対象（any の ID のリスト） |

## `select.all`

すべて選択

すべてを選択します（選択中にノートがあれば、そのクリップのノート。なければ全クリップ。--clip・--track で絞れます）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Mod+A (`select.all`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | — | `{"type":"string"}` | このクリップのノートを対象にします（clip の ID） |
| `track` | — | `{"type":"string"}` | このトラックのクリップを対象にします（track の ID） |

## `select.clear`

選択を解除

選択を解除します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Mod+D (`select.clear`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `select.get`

選択中のもの

現在の選択を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `select.in-loop`

ループ範囲のものを選択

ループ範囲にかかっているクリップを選択します（--clip を指定すると、そのクリップのノート）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Alt+L (`select.in-loop`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | — | `{"type":"string"}` | このクリップのノートを対象にします（clip の ID） |
| `track` | — | `{"type":"string"}` | このトラックのクリップを対象にします（track の ID） |

## `select.in-section`

セクションのものを選択

セクションにかかっているクリップを選択します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | セクション（section の ID） |

## `select.in_folder`

中のトラックを選択

フォルダーの中のトラックを選択します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | フォルダー（folder の ID のリスト） |

## `select.invert`

選択を反転

選択を反転します（選択中にノートがあれば、そのクリップのノートの中で。なければクリップの中で）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Mod+I (`select.invert`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `clip` | — | `{"type":"string"}` | このクリップのノートを対象にします（clip の ID） |
| `track` | — | `{"type":"string"}` | このトラックのクリップを対象にします（track の ID） |

## `select.off-tracks`

トラック上の選択を解除

トラックのクリップを選択から外します（ほかの選択は残ります）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: arrange: Mod+Shift+D (`select.off-tracks`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（省略時は選択中のトラック）（track の ID のリスト） |

## `select.on-tracks`

トラック上のクリップをすべて選択

選択中のトラック（選択中のクリップがあるトラックも）の、すべてのクリップを選択します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Mod+Shift+A (`select.on-tracks`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラックかクリップ（省略時は選択中のもの）（any の ID のリスト） |

## `select.pitch`

同じ高さのノートを選択

ピアノロールで同じ高さのノートを選択します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `pitch` | 必須 | `{"maximum":127.0,"minimum":0.0,"type":"integer"}` | ノート番号（0〜127） |

## `select.remove`

選択から除外

選択から除外します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `ids` | 必須 | `{"items":{"type":"string"},"type":"array"}` | 選択する対象（any の ID のリスト） |

## `select.set`

選択を置き換え

選択を指定したものだけにします

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `ids` | 必須 | `{"items":{"type":"string"},"type":"array"}` | 選択する対象（any の ID のリスト） |

## `select.sources`

送っているチャンネルを選択

バスに送っているチャンネルを選択します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | バス（track の ID） |

## `select.span`

先頭・最後まで選択

選択中のクリップから曲の先頭（--way start）か最後（--way end）まで、同じトラックのクリップを選択します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: arrange: Alt+Shift+Home (`select.span --way start`)、arrange: Alt+Shift+End (`select.span --way end`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `way` | 必須 | `{"enum":["start","end"],"type":"string"}` | 広げる向き |

## `select.step`

選択を移動

選択を矢印の向きへ 1 つ動かします（--dir start / end は先頭と最後。--extend で選択を広げる）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Left (`select.step --dir left`)、global: Shift+Left / Mod+Shift+Left (`select.step --dir left --extend`)、global: Right (`select.step --dir right`)、global: Shift+Right / Mod+Shift+Right (`select.step --dir right --extend`)、global: Up (`select.step --dir up`)、global: Shift+Up / Mod+Shift+Up (`select.step --dir up --extend`)、global: Down (`select.step --dir down`)、global: Shift+Down / Mod+Shift+Down (`select.step --dir down --extend`)、global: Home (`select.step --dir start`)、global: Shift+Home / Mod+Shift+Home (`select.step --dir start --extend`)、global: End (`select.step --dir end`)、global: Shift+End / Mod+Shift+End (`select.step --dir end --extend`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dir` | 必須 | `{"enum":["left","right","up","down","start","end"],"type":"string"}` | 向き |
| `extend` | — | `{"type":"boolean"}` | 選択を保ったまま広げる |

## `select.using_file`

使っているクリップを選択

ファイルを使うクリップを選択します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `file` | 必須 | `{"type":"string"}` | ファイル（file.list の file） |

## `send.add`

センドを追加

バスへのセンドを追加します（既定はポストフェーダー）。--to にプラグインを指定すると、その機器のサイドチェーンの送り元を追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `send`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 送り元のトラック（track の ID のリスト） |
| `level` | — | `{"default":0.0,"maximum":12.0,"minimum":-120.0,"type":"number"}` | センド量（dB。--to がプラグインのときは -60〜6） |
| `pre` | — | `{"type":"boolean"}` | プリフェーダーで送る |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `to` | 必須 | `{"type":"string"}` | 送り先のバス、またはキーを受けるプラグイン（track\|plugin の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `send.add.bus`

新しいバスへのセンドを追加

新しいバスを作成し、そのバスへのセンドを追加します（1 回の取り消しで両方とも元に戻ります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `bus`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 送り元のトラック（track の ID のリスト） |
| `into` | — | `{"default":"root","type":"string"}` | バスの追加先のフォルダーの ID（root または省略時は最上位） |
| `level` | — | `{"default":0.0,"maximum":12.0,"minimum":-120.0,"type":"number"}` | センド量（dB） |
| `name` | — | `{"type":"string"}` | バスの名前（省略時は自動で付けます） |
| `pre` | — | `{"type":"boolean"}` | プリフェーダーで送る |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `send.level.reset`

センドの量を 0 dB に戻す

センドの量を 0 dB に戻します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `send`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | センド（send の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `send.mute`

ミュート

センドのミュートを切り替えます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `send`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | センド（send の ID） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `send.pre`

プリフェーダー

センドのプリフェーダーを切り替えます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `send`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | センド（send の ID） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `send.remove`

センドを削除

センドを削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `send`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | センド（send の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `send.set`

センドを設定

センドの量やプリ／ポストを変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `send`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | センド（send の ID） |
| `level` | — | `{"maximum":12.0,"minimum":-120.0,"type":"number"}` | センド量（dB） |
| `position` | — | `{"enum":["pre","post"],"type":"string"}` | プリフェーダーかポストフェーダーか |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `send.target.set`

送り先

センドの送り先を変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `send`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | センド（send の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `to` | 必須 | `{"type":"string"}` | 送り先のバスの ID または new |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `snap.invert`

スナップを一時反転

ドラッグ中のスナップを一時反転

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `song.recent`

最近の曲

最近開いた曲の一覧と、次の起動で開く曲を表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `take.activate`

このテイクを使用

重なっているテイクのうち、このテイクだけを再生します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テイク（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `take.clean`

未使用のテイクをすべて削除

この範囲で使っていないテイクを削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テイク（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `take.comp`

コンピング

指定した範囲で、選んだテイクだけが再生されるようにします（範囲の両端でテイクを分割し、重なっている他のテイクをミュートします。1 回で取り消せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `from` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `take` | 必須 | `{"type":"string"}` | 再生するテイク（そのテイクのクリップのいずれか。以前の範囲で分割されていてもよい）（clip の ID） |
| `to` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `take.delete`

テイクを削除

テイクを削除します

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テイク（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `take.next`

次のテイク

重なっているテイクのうち、次のテイクを再生します（最後の次は最初）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: arrange: Alt+H (`take.next`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テイク（重なっているもののいずれか）（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `take.prev`

前のテイク

重なっているテイクのうち、前のテイクを再生します（最初の前は最後）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

キー: arrange: Alt+G (`take.prev`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テイク（重なっているもののいずれか）（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `take.undo-lap`

最後の周を取り消す

重なっているテイクのうち、最後の周（いちばん下のテイク）を取り消します（取り消したテイクだけが再生されていたときは、ひとつ前のテイクを再生します）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テイク（重なっているもののいずれか）（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `take.unpack`

テイクを新しいトラックに展開

テイクを新しいトラックに展開します（元のテイクはそのまま残ります。1 回で取り消せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テイク（clip の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `tempo.clear`

テンポの変化をすべて削除

テンポの変化をすべて消します（曲の先頭のテンポだけが残ります）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `tempo`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `tempo.delete`

テンポポイントを削除

テンポポイントを削除します（曲の先頭のポイントは削除できません）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テンポポイント（tempo の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `tempo.draw`

テンポに形を描く

テンポの範囲に、形を描きます（曲の先頭のポイントは動かしません）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `tempo`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `amp` | — | `{"type":"number"}` | 波の振幅（矩形波・三角波・ノコギリ波・サイン波。省略時は範囲の 1/4） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `end-value` | — | `{"type":"number"}` | 終了位置の値（省略時は開始位置と同じ） |
| `from` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の開始位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `period` | — | `{"maximum":1000000000.0,"minimum":1.0,"type":"integer"}` | 波の 1 周期の長さ（tick。省略時は 1 拍） |
| `phase` | — | `{"type":"integer"}` | 波の位相のずれ（tick。省略時は 0） |
| `points` | — | `{"items":{"type":"object"},"type":"array"}` | フリーハンドの軌跡 [{tick, value}] の配列 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `shape` | 必須 | `{"enum":["free","line","parabola","square","triangle","saw","sine"],"type":"string"}` | 形 |
| `step` | — | `{"maximum":1000000000.0,"minimum":1.0,"type":"integer"}` | ポイントの間隔（tick）。フリーハンドはこの間隔で読み直し、省略すると間引く |
| `to` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 範囲の終了位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `value` | — | `{"type":"number"}` | 開始位置の値（フリーハンドでは不要） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `tempo.move`

テンポポイントを移動

テンポポイントの位置とBPMを変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `tempo`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 位置（小節.拍.tick（例: 3.1.0）または tick 数） |
| `bpm` | — | `{"maximum":999.0,"minimum":1.0,"type":"number"}` | BPM |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | テンポポイント（tempo の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `tempo.set`

テンポを設定

テンポを設定します。指定位置にテンポポイントがあれば変更し、無ければ追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `tempo`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 位置（省略時は曲の先頭）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `bpm` | 必須 | `{"maximum":999.0,"minimum":1.0,"type":"number"}` | BPM（1〜999） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ramp` | — | `{"type":"boolean"}` | 次のテンポポイントまでなめらかに変化させる |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `timesig.delete`

拍子の変更を削除

拍子の変更点を削除します（1 小節目の拍子は削除できません）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | 拍子の変更点（timesig の ID） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `timesig.set`

拍子を設定

拍子を設定します。指定した小節に拍子の変更点があれば変更し、無ければ追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `bar` | — | `{"default":1,"maximum":100000.0,"minimum":1.0,"type":"integer"}` | 小節（省略時は 1） |
| `den` | 必須 | `{"type":"integer"}` | 1 拍の音符の長さ（1・2・4・8・16・32 分音符） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `num` | 必須 | `{"maximum":64.0,"minimum":1.0,"type":"integer"}` | 拍数 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `tool.alt`

代わりの道具を選択

矢印のまま Ctrl を押している間になる「代わりの道具」を選びます（省略時は次の道具。上のバーでは青い下線が付きます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `area` | — | `{"enum":["arrange","roll"],"type":"string"}` | どの画面か（省略時はアレンジ） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tool` | — | `{"enum":["range","cut","erase","draw","mute","listen"],"type":"string"}` | 代わりの道具 |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `tool.cut`

分割ツール

分割ツール

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: 3 (`tool.cut`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `tool.draw`

鉛筆ツール

鉛筆ツール

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: 5 (`tool.draw`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `tool.draw.shape`

鉛筆の形を選択

鉛筆が描く形を選びます（--shape で指定、--next・--prev で順に切り替えます）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `next` | — | `{"type":"boolean"}` | 次の形にする |
| `prev` | — | `{"type":"boolean"}` | 前の形にする |
| `shape` | — | `{"enum":["free","line","parabola","square","triangle","saw","sine","transform"],"type":"string"}` | 形 |

## `tool.erase`

消しゴムツール

消しゴムツール

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: 4 (`tool.erase`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `tool.listen`

試聴ツール

試聴ツール

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: 8 (`tool.listen`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `tool.mute`

ミュートツール

ミュートツール

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: 6 (`tool.mute`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `tool.range`

範囲ツール

範囲ツール

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: 2 (`tool.range`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `tool.select`

選択ツール

選択ツール

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: 1 (`tool.select`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `track.add.audio`

オーディオトラックを追加

オーディオトラックを追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `wave`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `into` | — | `{"default":"root","type":"string"}` | 追加先のフォルダーの ID（root または省略時は最上位） |
| `name` | — | `{"type":"string"}` | 名前（省略時は自動で付けます） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.add.bus`

バスを追加

バスを追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `bus`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `format` | — | `{"default":"builtin","type":"string"}` | --plugin の形式（vst3・clap・builtin など） |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `into` | — | `{"default":"root","type":"string"}` | 追加先のフォルダーの ID（root または省略時は最上位） |
| `name` | — | `{"type":"string"}` | 名前（省略時は自動で付けます） |
| `plugin` | — | `{"type":"string"}` | 新しいバスに追加するエフェクトの名前（1 つの操作です） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `sources` | — | `{"items":{"type":"string"},"type":"array"}` | まとめるチャンネル（track の ID のリスト） |
| `uid` | — | `{"type":"string"}` | --plugin の固有 ID（省略時は名前） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.add.instrument`

インストゥルメントトラックを追加

インストゥルメント（MIDI）トラックを追加します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `instrument`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `into` | — | `{"default":"root","type":"string"}` | 追加先のフォルダーの ID（root または省略時は最上位） |
| `name` | — | `{"type":"string"}` | 名前（省略時は自動で付けます） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.add.with-instrument`

インストゥルメント付きトラックを追加

インストゥルメントを入れたインストゥルメントトラックを追加します。--arm で録音準備にし、--notes を渡すとそのノートの MIDI クリップを置きます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `arm` | — | `{"type":"boolean"}` | 録音準備にする |
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | クリップの位置（省略時は曲の頭）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `clip` | — | `{"type":"string"}` | クリップの名前（省略時はトラック名） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `format` | — | `{"default":"builtin","type":"string"}` | vst3・clap・builtin など |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `instrument` | 必須 | `{"type":"string"}` | インストゥルメントの名前 |
| `into` | — | `{"default":"root","type":"string"}` | 追加先のフォルダーの ID（root または省略時は最上位） |
| `name` | — | `{"type":"string"}` | トラック名（省略時はインストゥルメントの名前） |
| `notes` | — | `{"items":{"type":"object"},"type":"array"}` | ノートの一覧（{at, pitch, length, velocity} の配列。at・length は「小節.拍.tick」または tick 数で、at はクリップの頭から） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `uid` | — | `{"type":"string"}` | 形式ごとの固有 ID（省略時は名前） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.arm`

録音準備

録音準備を切り替えます（--on / --off。省略時は現在の状態を反転）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `record`。

キー: edit: R (`track.arm`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.bounce`

新しいトラックにバウンス

トラックの音を、インサートを通して（ボリューム・パンの手前で）下の新しいトラックに置き、元のトラックをミュートします。新しいトラックのボリューム・パン・センド・出力は元と同じです

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.color.set`

色を設定

トラックかフォルダーの色を設定します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `color` | 必須 | `{"type":"string"}` | #rrggbb または none |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラックかフォルダー（track\|folder の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.delete`

トラックを削除

トラックを削除します（バスを削除すると、そのバスへの出力はマスターに戻ります）

事前条件: AI の権限が編集以上。削除の確認設定に従う。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `trash`。

キー: edit: Shift+T (`track.delete`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 削除するトラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.duplicate`

トラックを複製

トラックを複製します。--mode settings では設定だけをコピーします。--into を指定すると、そのフォルダーに配置します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 複製するトラック（track の ID のリスト） |
| `index` | — | `{"type":"integer"}` | --into のフォルダー内の位置（省略時は末尾） |
| `into` | — | `{"type":"string"}` | 複製の配置先のフォルダーの ID または root（省略時は元のトラックの直後） |
| `mode` | — | `{"default":"all","enum":["all","settings"],"type":"string"}` | 中身ごと、または設定だけ |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.freeze`

トラックをフリーズ

トラックの音を、インストゥルメントとインサートを通して（ボリューム・パンの手前で）ファイルに書き出し、以後はその音を鳴らします。フリーズ中のトラックのクリップ・インサートは編集できません。ボリューム・パン・センドは使えます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.freeze.apply`

フリーズした音を置く

フリーズした音をトラックに置きます（track.freeze が使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `tracks` | 必須 | `{"items":{"type":"object"},"type":"array"}` | トラック [{id, file, start}] |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.icon.set`

トラックのアイコンを設定

トラックのアイコンを設定します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `icon` | 必須 | `{"enum":["drums","keys","mic","wave","none"],"type":"string"}` | アイコン（none でトラックの種類の既定に戻す） |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.input.gain`

入力ゲインを設定

入力のゲイン（トリム）を dB で設定します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `db` | 必須 | `{"maximum":24.0,"minimum":-24.0,"type":"number"}` | dB（-24〜24） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.input.set`

入力を設定

入力を割り当てます（none で解除）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `input` | 必須 | `{"type":"string"}` | 入力の名前（none で解除） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.list`

トラックの一覧

トラックを上から順に一覧表示します（フォルダーの入れ子は depth で示します）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `track.monitor`

入力モニター

入力のモニター（録音準備中に入力の音を出力で聴く）を切り替えます（--on / --off。省略時は現在の状態を反転）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

キー: edit: U (`track.monitor`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.move`

トラックを移動

トラックやフォルダーを移動します。--into でフォルダーの中へ（root で最上位へ）、--out で 1 つ上の階層へ、--by で同じ階層の中で上下に移動

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"integer"}` | 同じ階層の中での移動数（負の値で上へ） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 移動するトラックかフォルダー（track\|folder の ID のリスト） |
| `index` | — | `{"type":"integer"}` | --into のフォルダー内の位置（省略時は末尾） |
| `into` | — | `{"type":"string"}` | 移動先のフォルダーの ID または root |
| `out` | — | `{"type":"boolean"}` | 1 つ上の階層（親フォルダーの直後）へ移動する |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.mute`

トラックをミュート

ミュートを切り替えます（--on / --off。省略時は現在の状態を反転）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `mute`。

キー: edit: M (`track.mute`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.mute.clear`

ミュートをすべて解除

すべてのトラックのミュートを解除します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

キー: global: Mod+Shift+M (`track.mute.clear`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.output.set`

出力先を設定

出力先を設定します（master・バスの ID・default＝フォルダーの設定に従う・midi:ポート名＝MIDI 出力ポート）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `channel` | — | `{"type":"integer"}` | midi: のときの MIDI チャンネル（1〜16。省略時は 1） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `to` | 必須 | `{"type":"string"}` | master・バスの ID・default・midi:ポート名 のいずれか |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.pan.set`

パンを設定

パンを設定します（-1 が左、1 が右）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `pan` | 必須 | `{"maximum":1.0,"minimum":-1.0,"type":"number"}` | -1〜1 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.preset.add`

プリセットからトラックを追加

プリセットから新しいトラックを作ります（名前はプリセットの名前になります）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `into` | — | `{"default":"root","type":"string"}` | 追加先のフォルダーの ID（root または省略時は最上位） |
| `preset` | 必須 | `{"type":"string"}` | トラックのプリセットの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.preset.apply`

トラックのプリセットを適用

プリセットをトラックに適用します（名前・入力・出力・クリップは変わりません。1 回の取り消しで戻せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 適用するトラック（track の ID のリスト） |
| `preset` | 必須 | `{"type":"string"}` | トラックのプリセットの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.preset.list`

トラックのプリセットの一覧

保存したトラックのプリセットを一覧します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `track.preset.make`

プリセットのトラックを作成

読み込んだトラックのプリセットから、新しいトラックを作ります（track.preset.add から使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `index` | — | `{"type":"integer"}` | フォルダー内の位置（0 が先頭。省略時は末尾） |
| `into` | — | `{"default":"root","type":"string"}` | 追加先のフォルダーの ID（root または省略時は最上位） |
| `missing` | — | `{"type":"string"}` | この PC に無いプラグインの uid（コンマ区切り。空のスロットとして残します） |
| `preset` | 必須 | `{"type":"string"}` | プリセットの内容（JSON） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.preset.origin`

プリセットの出どころを記録

トラックが、どのプリセットから作られたか（保存したか）を記録します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `name` | 必須 | `{"type":"string"}` | プリセットの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.preset.put`

プリセットの内容を反映

読み込んだトラックのプリセットの内容を、トラックに反映します（track.preset.apply から使います）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | 反映するトラック（track の ID のリスト） |
| `missing` | — | `{"type":"string"}` | この PC に無いプラグインの uid（コンマ区切り。空のスロットとして残します） |
| `preset` | 必須 | `{"type":"string"}` | プリセットの内容（JSON） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.preset.save`

トラックのプリセットを保存

トラックの設定（エフェクト・センド・ボリューム・パン・色・インストゥルメント）をプリセットとして保存します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | プリセットの名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | 必須 | `{"type":"string"}` | 保存するトラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.preset.update`

トラックのプリセットを更新

トラックの今の設定で、そのトラックの元のプリセットを上書きします

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `track` | 必須 | `{"type":"string"}` | 更新するトラック（track の ID） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.rename`

トラック名を変更

トラックの名前を変更します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `name` | 必須 | `{"type":"string"}` | 新しい名前 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.reveal`

チャンネルを表示

チャンネルをトラックの列かミキサーで見せます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `keys`。

キー: global: Mod+Alt+T (`track.reveal`)、global: Mod+Alt+C (`track.reveal --in mixer`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `id` | 必須 | `{"type":"string"}` | トラック（track の ID） |
| `in` | — | `{"default":"tracks","enum":["tracks","mixer"],"type":"string"}` | 見せる場所（省略時はトラックの列） |

## `track.solo`

トラックをソロ

ソロを切り替えます（--on / --off。省略時は現在の状態を反転）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `solo`。

キー: edit: S (`track.solo`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.solo.clear`

ソロをすべて解除

すべてのトラックのソロを解除します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

キー: global: Mod+Shift+S (`track.solo.clear`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.transpose`

トラックをトランスポーズ

トラックの音の高さを半音で設定します（画面のノートは動かさず、鳴らすときに足します。クリップのトランスポーズと足し合わせます）。範囲（−64〜64）の外は丸めます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `semis` | 必須 | `{"maximum":64.0,"minimum":-64.0,"type":"integer"}` | トランスポーズ（半音。範囲の外は丸めます） |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.unfreeze`

フリーズを解除

フリーズを解除し、トラックのクリップ・インストゥルメント・インサートをまた鳴らします

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.velocity`

トラックのベロシティを設定

トラックのベロシティを % で設定します（画面のノートは変えず、鳴らすときに足します。−100% で 0、+100% で 2 倍になり、1〜127 に丸めます）。範囲（−100〜100）の外は丸めます

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `percent` | 必須 | `{"maximum":100.0,"minimum":-100.0,"type":"integer"}` | ベロシティ（%。範囲の外は丸めます） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `track.volume.set`

ボリュームを設定

ボリューム（フェーダー）を dB で設定します

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `keys`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `db` | 必須 | `{"maximum":12.0,"minimum":-120.0,"type":"number"}` | dB（-120〜12） |
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | トラック（track の ID のリスト） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `transport.goto`

再生位置をジャンプ

再生位置を、曲の頭・終わり・ループの端・マーカー・拍や小節の線・クリップの端へ移動します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Shift+N (`transport.goto --to next-marker`)、global: Shift+B (`transport.goto --to prev-marker`)、global: NumPad1 (`transport.goto --to loop-start`)、global: NumPad2 (`transport.goto --to loop-end`)、global: NumPad- / MediaRewind (`transport.goto --to prev-beat`)、global: NumPad+ / MediaForward (`transport.goto --to next-beat`)、global: Shift+NumPad- / Shift+MediaRewind (`transport.goto --to prev-bar`)、global: Shift+NumPad+ / Shift+MediaForward (`transport.goto --to next-bar`)、global: Alt+Shift+B (`transport.goto --to prev-event`)、global: Alt+Shift+N (`transport.goto --to next-event`)、global: NumPad3 (`transport.goto --to marker-1`)、global: NumPad4 (`transport.goto --to marker-2`)、global: NumPad5 (`transport.goto --to marker-3`)、global: NumPad6 (`transport.goto --to marker-4`)、global: NumPad7 (`transport.goto --to marker-5`)、global: NumPad8 (`transport.goto --to marker-6`)、global: NumPad9 (`transport.goto --to marker-7`)、global: Mod+NumPad. (`transport.goto`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `to` | 必須 | `{"type":"string"}` | 移動先: start・end・loop-start・loop-end・next-marker・prev-marker・next-beat・prev-beat・next-bar・prev-bar・next-event・prev-event、または marker-1 のようなマーカーの番号 |

## `transport.locate`

位置を指定して移動…

移動先の位置を入力する枠を開きます（初めは再生位置）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+T (`transport.locate`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `transport.locate-mouse`

ポインターに移動

ポインターに移動

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+Space (`transport.locate-mouse`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `transport.locate-selection`

選択範囲に移動

選択範囲に移動

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: L (`transport.locate-selection`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `transport.loop`

ループ

ループ再生を切り替えます（範囲は loop.set で設定）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `loop`。

キー: global: NumPad/ / / (`transport.loop`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |

## `transport.metronome`

メトロノーム

メトロノーム

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: C (`transport.metronome`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `transport.panic`

オールノートオフ

鳴っているノートをすべて止めます（オールノートオフ）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: # (`transport.panic`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `transport.play`

再生 / 一時停止

再生します（--at で開始位置を指定、--loop でループ範囲を繰り返し再生）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `play`。

キー: global: Space / MediaPlayPause (`transport.play`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | — | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 再生の開始位置（省略時は現在の位置）（小節.拍.tick（例: 3.1.0）または tick 数） |
| `loop` | — | `{"type":"boolean"}` | ループ範囲を繰り返し再生する |

## `transport.seek`

再生位置を移動

再生位置を移動します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `skip-back`。

キー: global: NumPad. / , (`transport.seek --at 0`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 移動先（小節.拍.tick（例: 3.1.0）または tick 数） |

## `transport.start`

再生を開始

再生を開始

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Enter (`transport.start`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `transport.status`

再生の状態

再生状態・現在の位置・ループを表示します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `transport.stop`

停止

再生を停止します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `stop`。

キー: global: NumPad0 / MediaStop (`transport.stop`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `version.list`

バージョンの一覧…

バージョンを一覧表示します（名前付きと自動保存）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `history`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `version.open`

バージョンを別の曲として開く

バージョンを別の曲として開きます（現在の曲はそのまま）

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | バージョンの ID（v1 など） |
| `path` | 必須 | `{"type":"string"}` | 新しい曲のフォルダー |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `version.restore`

バージョンに戻す

曲をバージョンの状態に戻します（これも 1 つの操作として履歴に残り、取り消せます）

事前条件: AI の権限が編集以上（読み取りではプレビューのみ）。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 1 操作で取り消し可能。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `id` | 必須 | `{"type":"string"}` | バージョンの ID（v1 など） |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `version.save`

バージョンを保存…

名前を付けたバージョンを保存します

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `saved`。

キー: global: Mod+Alt+Shift+S (`version.save`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `name` | 必須 | `{"type":"string"}` | バージョン名 |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `view.add-track`

トラックを追加…

トラックを追加…

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: T (`view.add-track`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.autoscroll`

自動スクロール

自動スクロール

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: F (`view.autoscroll`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.browser`

ブラウザーを表示

ブラウザーを表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: F5 (`view.browser`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `tab` | — | `{"enum":["instruments","effects","files","song_files"],"type":"string"}` | 開くタブ（省略時はブラウザーを開閉します） |

## `view.browser.hidden`

隠したものを表示

隠したインストゥルメントやエフェクトも一覧に表示します（--off で表示しません）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |

## `view.browser.sort`

ブラウザーの表示順

ブラウザーの一覧の並べ方を変えます（プールは kind と name だけ）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `order` | 必須 | `{"enum":["kind","vendor","format","name","favourites"],"type":"string"}` | 並べ方 |
| `tab` | — | `{"enum":["instruments","effects","song_files"],"type":"string"}` | タブ（省略時は表示中のタブ） |

## `view.channel.width`

チャンネルの幅

ミキサーのチャンネル幅を変更します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `width` | 必須 | `{"enum":["narrow","wide"],"type":"string"}` | 幅 |

## `view.cleanup`

使われていない録音を整理…

使われていない録音のファイルと合計の大きさを一覧にして、ごみ箱に移動する前に確かめます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.context-menu`

コンテキストメニューを開く

コンテキストメニューを開く

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: arrange: Shift+F10 / Menu (`view.context-menu`)、roll: Shift+F10 / Menu (`view.context-menu`)、mixer: Shift+F10 / Menu (`view.context-menu`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.dock`

ドックを開閉

下のドック（ピアノロール・ミキサー）を開閉します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `state` | — | `{"default":"toggle","enum":["open","closed","toggle"],"type":"string"}` | 開くか閉じるか（省略時は切り替え） |
| `tab` | 必須 | `{"enum":["roll","mixer"],"type":"string"}` | ドックのタブ |

## `view.export`

書き出しの画面を開く

書き出しの画面を開きます（--ids を渡すとそのステム、省略時は --range のミックスダウン）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `ids` | — | `{"items":{"type":"string"},"type":"array"}` | ステムにするトラック（フォルダー 1 つならその 1 本のステム）（track\|folder の ID のリスト） |
| `kind` | — | `{"type":"string"}` | midi で MIDI ファイル（--ids はそのインストゥルメントのトラック） |
| `range` | — | `{"default":"song","type":"string"}` | song・loop・a..b（位置の範囲） |

## `view.export-run`

書き出しを開始

書き出しを開始

事前条件: AI の権限が保存以上、またはユーザーの承認。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 実行結果の step で確認。アイコン: `prompt`。

キー: export: Mod+Enter (`view.export-run`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `dry_run` | — | `{"type":"boolean"}` | 曲は変更せず、変更内容だけを返します |
| `request_key` | — | `{"type":"string"}` | 再送時にも同じ操作キーを使います |
| `wait_ms` | — | `{"maximum":30000,"minimum":0,"type":"integer"}` | 結果を待つ時間。0 は受付後すぐに返します |

## `view.export-stems`

ステムを書き出す…

ステムを書き出す…

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+Shift+E (`view.export-stems`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.export.close`

書き出しの画面を閉じる

書き出しの画面を閉じます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.focus`

フォーカスを移動

キーの効く場所を、アレンジ・下のドック・ブラウザーの順に次へ移します（--way prev で前へ）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Tab (`view.focus --way next`)、global: Shift+Tab (`view.focus --way prev`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `way` | — | `{"default":"next","enum":["next","prev"],"type":"string"}` | 移す向き |

## `view.folders`

フォルダーをすべて開閉

すべてのフォルダーの開閉を変更します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `state` | 必須 | `{"enum":["open","closed"],"type":"string"}` | 開閉 |

## `view.fullscreen`

全画面表示

窓を全画面にする・元に戻します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Shift+F (`view.fullscreen`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.grid`

グリッドを設定

編集グリッドを設定します（1/16・1/8t・1/4d など）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `editor` | — | `{"enum":["arrange","roll"],"type":"string"}` | エディター |
| `grid` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | グリッド（1/16・1/8t（3 連符）・1/4d（付点）または tick 数） |

## `view.groove.import`

グルーヴを読み込む

外部グルーヴを読み込みます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `hits` | 必須 | `{"items":{"type":"object"},"type":"array"}` | [{at,velocity}] |
| `name` | 必須 | `{"type":"string"}` | グルーヴ名 |
| `period` | 必須 | `{"maximum":9.223372036854776e+18,"minimum":1.0,"type":"integer"}` | 周期（tick） |

## `view.instrument`

インストゥルメントを表示

インストゥルメントを表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Shift+F11 (`view.instrument`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.lanes`

オートメーションを表示

オートメーションを表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: A (`view.lanes`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.link`

Link を切り替え

アレンジの Link を切り替えます

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |

## `view.mixer`

ミキサーを表示

ミキサーを表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: F3 (`view.mixer`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.page`

ページ送り

トラックの列を 1 画面ぶん上（--way up）か下（--way down）へスクロールします

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: PageUp (`view.page --way up`)、edit: PageDown (`view.page --way down`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `way` | 必須 | `{"enum":["up","down"],"type":"string"}` | スクロールの向き |

## `view.palette`

コマンドパレット

コマンドパレット

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+K (`view.palette`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.pc-keyboard`

PC キーボードで演奏

PC のキーボードで演奏するのを入・切します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: CapsLock (`view.pc-keyboard`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.record-panel`

録音パネルを表示

録音パネルを表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Alt+Shift+R (`view.record-panel`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.ruler`

ルーラーの目盛り

ルーラーの目盛りを変更します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `unit` | 必須 | `{"enum":["bars","seconds"],"type":"string"}` | 目盛り |

## `view.scale`

スケールを設定

ピアノロールのスケールを設定します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `root` | — | `{"default":0,"maximum":11.0,"minimum":0.0,"type":"integer"}` | ルートの音名（C=0） |
| `scale` | — | `{"default":"major","enum":["major","minor","off"],"type":"string"}` | スケール |

## `view.scroll`

表示位置を移動

表示の左端の位置を設定します

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `at` | 必須 | `{"oneOf":[{"type":"integer"},{"type":"string"}]}` | 位置（小節.拍.tick（例: 3.1.0）または tick 数） |

## `view.settings`

設定

設定

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+, (`view.settings`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.snap`

スナップ

グリッドへのスナップを切り替えます（--on / --off。省略時は現在の状態を反転）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `grid`。

キー: edit: N / Mod+Shift+G (`view.snap`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `editor` | — | `{"enum":["arrange","roll"],"type":"string"}` | エディター |
| `off` | — | `{"type":"boolean"}` | オフ |
| `on` | — | `{"type":"boolean"}` | オン（省略時は現在の状態を切り替え） |

## `view.tracklist.collapse`

トラック一覧を畳む

トラック一覧を畳む

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: global: Mod+Shift+L (`view.tracklist.collapse`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.zoom`

ズーム

表示のズーム倍率を設定します（横・縦）

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `h` | — | `{"maximum":1000.0,"minimum":0.01,"type":"number"}` | 横の倍率 |
| `v` | — | `{"maximum":1000.0,"minimum":0.01,"type":"number"}` | 縦の倍率 |

## `view.zoom.data`

縦のズーム

ズームを変更

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Shift+E / Mod+Shift+NumPad+ (`view.zoom.data --by 1`)、edit: Shift+W / Mod+Shift+NumPad- (`view.zoom.data --by -1`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | 必須 | `{"type":"number"}` | 段数 |

## `view.zoom.full`

全体を表示

全体を表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Alt+Z (`view.zoom.full`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.zoom.loop`

ループを表示

ループを表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Shift+L (`view.zoom.loop`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.zoom.rect`

囲んだ範囲を表示

囲んだ範囲を表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.zoom.redo`

ズームをやり直す

ズームをやり直す

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Alt+E (`view.zoom.redo`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.zoom.selection`

選択範囲を表示

選択範囲を表示

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Shift+S (`view.zoom.selection`)、edit: Alt+S (`view.zoom.selection --horizontal`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `horizontal` | — | `{"type":"boolean"}` | 横だけ |

## `view.zoom.store`

ズームを記憶

ズームを記憶

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Shift+Z (`view.zoom.store`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.zoom.time`

横のズーム

ズームを変更

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: E / Mod++ / Mod+NumPad+ (`view.zoom.time --by 1`)、edit: W / Mod+- / Mod+NumPad- (`view.zoom.time --by -1`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"number"}` | 段数 |
| `to` | — | `{"type":"number"}` | 1 拍の幅（px、8 から、1 サンプルが 6 px になる幅まで）。見ている中央の位置を動かさずに合わせます |

## `view.zoom.toggle`

ズームを切り替え

ズームを切り替え

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Z (`view.zoom.toggle`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.zoom.undo`

ズームを戻す

ズームを戻す

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

キー: edit: Alt+W / Alt+Backspace (`view.zoom.undo`)。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|

## `view.zoom.wave`

波形の縦の拡大

波形の縦の拡大

事前条件: AI の権限が読み取り以上。指定した ID が存在し、引数が次の型・範囲に一致すること。

履歴: 記録なし。アイコン: `prompt`。

| 引数 | 必須 | 型・範囲・既定値 | 内容 |
|---|---|---|---|
| `by` | — | `{"type":"number"}` | 段数（×1・×2・×4 を上下） |
| `to` | — | `{"type":"number"}` | 倍率（1・2・4） |

