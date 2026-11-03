# ARA API 2.3.0（LIC-004）

Celemony の [ARA_SDK](https://github.com/Celemony/ARA_SDK) の `releases/2.3.0`（commit `0005c532`）の
`ARA_API`（commit `65ec5c43`）から、次だけを写した。中身は直さない。

- `ARAInterface.h`・`ARAVST3.h`・`ARACLAP.h`・`ARAAudioFileChunks.h`
- `LICENSE.txt`（Apache License 2.0。`ARA_API` の物）
- `NOTICE.txt`（ARA_SDK のルートの物）

Rust の束縛 `crates/xirius-ara/src/sys.rs` は `ARAInterface.h`・`ARAVST3.h` を手で写した物で、
アプリに組み込まれる。そのため、このフォルダーは `scripts/licenses.py` の `VENDORED` に載せ、
ライセンスの文を `THIRD-PARTY-LICENSES.txt` に、出どころを `NOTICE` に書く。

写さない物: `ARA_Library`（C++）・`ARA_Examples`・`3rdParty`（cpp-base64・pugixml・ICST AudioFile）・
`install_JUCE_ARA.cmake`（JUCE を取るため禁止。`docs/licenses.md`）・`ARAAudioUnit.h`（macOS の AU だけ。
macOS で AU を結ぶときに足す）。

ARA_Examples の TestHost・TestPlugIn は試験のためだけに手元で作る（`docs/ara-host.md`）。
