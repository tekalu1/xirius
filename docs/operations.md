# 画面の操作の一覧

UI-004: 同じ操作を画面の 2 か所に置かない。この一覧は `crates/xirius-ui/src/operations.rs` の
試験が画面から読んだ結果で、手で直さない（`XIRIUS_WRITE_OPERATIONS=1 cargo test -p xirius-ui operations`
で書き直す）。2 か所以上にある操作は、承認したモックが両方を描くものだけで、下の「承認済みの重複」に根拠を書く。

## 承認済みの重複

| 操作 | 場所 | 根拠 |
| --- | --- | --- |
| `export.open` | `bar.export`, `popup.song` | 上の帯の書き出しボタンと曲のメニューの「書き出し」: モックの daw-main.html が両方を描く（rev2-spec §13） |
| `version.list` | `bar.saved`, `popup.song` | 保存の札と曲のメニューの「バージョン一覧…」: モックが両方を描く（rev4-spec §5） |
| `record.preroll` | `bar.preroll`, `rec.preroll` | 下の帯のプリロールのボタンと録音パネルのスイッチ: モックの recording.html の 04 が両方を描く（ボタンは UI-300 で下の帯へ移した） |
| `view.snap` | `bar.snap`, `popup.grid` | 上の帯のスナップのスイッチとグリッドのメニューのオン・オフ: モックの ui-feedback.html の 04（B）が、スイッチを分け、値のボタンは今のグリッドのメニューを開く形にした（UI-302） |
| `track.mute` | `head.mute`, `mixer.mute` | トラックの頭とミキサーの帯の M: モックの daw-main.html が両方を描く（同じトラックの同じ状態を指す） |
| `track.solo` | `head.solo`, `mixer.solo` | トラックの頭とミキサーの帯の S: 同上 |
| `track.arm` | `head.arm`, `mixer.arm` | トラックの頭とミキサーの帯の R: 同上 |
| `track.output.menu` | `head.output`, `mixer.output` | トラックの頭（広げたとき）とミキサーの帯の出力: 同上 |

## すべての操作

| 操作 | 場所 |
| --- | --- |
| `about.open` | `bar.logo` |
| `about.panel` | `popup.about` |
| `ai.menu` | `bar.ai` |
| `ai.panel` | `popup.ai` |
| `audio.devices` | `start.device` |
| `browser.fav.toggle` | `browser.fav` |
| `browser.group.fold` | `browser.group` |
| `browser.item.pick` | `browser.item` |
| `browser.loc.add` | `browser.loc_add` |
| `browser.loc.pick` | `browser.loc` |
| `browser.pool.menu` | `browser.pool_menu` |
| `browser.pool.pick` | `browser.pool` |
| `browser.search` | `browser.search` |
| `browser.tab` | `rail.tab` |
| `browser.view.menu` | `browser.view` |
| `browser.view.set` | `popup.browserview` |
| `clip.select` | `clip` |
| `cpu.list` | `bar.cpu` |
| `drawer.tab` | `drawer.tab` |
| `edit.ripple` | `bar.ripple` |
| `export.dawproject` | `popup.song` |
| `export.open` | `bar.export`, `popup.song` |
| `export.panel` | `popup.export` |
| `folder.bus.menu` | `head.bus` |
| `folder.bus.set` | `popup.folderbus` |
| `folder.fold` | `fold` |
| `grid.menu` | `bar.grid` |
| `history.jump` | `popup.history` |
| `history.menu` | `bar.history` |
| `import.file` | `popup.addtrack` |
| `insert.add` | `mixer.add_insert` |
| `insert.pick` | `popup.insert` |
| `inspector.value` | `inspector.field` |
| `instrument.add` | `mixer.add_instrument` |
| `instrument.open` | `head.keys` |
| `instrument.pick` | `popup.instrument` |
| `meter.menu` | `bar.meter` |
| `palette.open` | `bar.palette` |
| `prefs.set` | `drawer.roll.length` |
| `project.settings` | `popup.song` |
| `quantize.panel` | `popup.grid` |
| `record.count-in` | `rec.count` |
| `record.count-in.bars` | `rec.count_bars` |
| `record.input-quantize` | `rec.quantize` |
| `record.input-quantize.menu` | `rec.quantize_value` |
| `record.input-quantize.set` | `popup.recquantize` |
| `record.postroll` | `rec.postroll` |
| `record.postroll.bars` | `rec.postroll_bars` |
| `record.preroll` | `bar.preroll`, `rec.preroll` |
| `record.preroll.bars` | `rec.preroll_bars` |
| `record.punch` | `rec.punch` |
| `roll.view` | `drawer.roll.view` |
| `row.select` | `row` |
| `send.add` | `mixer.add_send` |
| `send.pick` | `popup.send` |
| `settings.open` | `rail.settings` |
| `song.menu` | `bar.song` |
| `song.new` | `popup.song` |
| `song.open` | `popup.song` |
| `song.open_dawproject` | `popup.song` |
| `song.open_sample` | `start.sample` |
| `song.save` | `popup.song` |
| `song.save_as` | `popup.song` |
| `start.audio` | `start.audio` |
| `start.midi` | `start.midi` |
| `tempo.set` | `bar.tempo` |
| `timesig.set` | `popup.meter` |
| `tool.alt.menu` | `bar.alt_tool` |
| `tool.draw.shape.menu` | `bar.shape` |
| `tool.pick` | `bar.tool` |
| `track.add` | `popup.addtrack` |
| `track.add.menu` | `head.add` |
| `track.arm` | `head.arm`, `mixer.arm` |
| `track.color.menu` | `head.color` |
| `track.color.set` | `popup.color` |
| `track.freeze` | `head.freeze` |
| `track.input.menu` | `mixer.input` |
| `track.input.set` | `popup.input` |
| `track.monitor` | `head.monitor` |
| `track.mute` | `head.mute`, `mixer.mute` |
| `track.output.menu` | `head.output`, `mixer.output` |
| `track.output.set` | `popup.output` |
| `track.pan` | `mixer.pan` |
| `track.select` | `mixer.strip` |
| `track.solo` | `head.solo`, `mixer.solo` |
| `track.volume` | `mixer.fader` |
| `track.widen` | `head.widen` |
| `transport.loop` | `bar.loop` |
| `transport.metronome` | `bar.metronome` |
| `transport.play` | `bar.play` |
| `transport.record` | `bar.record` |
| `transport.seek` | `ruler` |
| `transport.stop` | `bar.stop` |
| `transport.to_start` | `bar.to_start` |
| `version.list` | `bar.saved`, `popup.song` |
| `view.arrange.toggle` | `head.arrange` |
| `view.auto_quantize` | `popup.grid` |
| `view.follow` | `head.follow` |
| `view.grid` | `popup.grid` |
| `view.record-panel` | `bar.rec_panel` |
| `view.snap` | `bar.snap`, `popup.grid` |
| `view.snap_mode` | `popup.grid` |
| `view.tempo.toggle` | `head.tempo` |
| `view.tracklist.collapse` | `head.fold` |
| `view.triplet` | `popup.grid` |
| `view.zoom.in` | `head.zoom_in` |
| `view.zoom.out` | `head.zoom_out` |
| `view.zoom.wave` | `head.wave_zoom` |

操作 108 件、うち 2 か所にあるもの 8 件（すべて承認済み）。

保存（UI-288）: Ctrl+S と曲のメニューは同じ保存を開始する。場所が未定なら別名保存の窓を開く。書き込み中の印は少なくとも 450 ms 保ち、200 ms を超える待ちは幕に載せる。別名保存の幕だけに進捗の棒と「中止 Esc」を置く。保存の警告の札の「別名で保存…」は同じ窓を開き、「もう一度」は同じ保存を再試行する。札を閉じても、保存成功まで警告の印を保つ。

ほかのアプリの変更（UI-289）: 札の「元に戻す」は Ctrl+Z（`history.undo`）と同じ操作で、「読み込む」は札だけにある。読み込みは履歴の 1 段「ほかのアプリの変更」として入り、元に戻す・やり直しで行き来する。
