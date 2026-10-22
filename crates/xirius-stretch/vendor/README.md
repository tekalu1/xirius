# 同梱のヘッダー

- `signalsmith-stretch/`: Signalsmith Stretch（MIT、Copyright (c) 2022 Geraint Luff / Signalsmith Audio Ltd.）
- `signalsmith-linear/`: Signalsmith Linear（MIT、Copyright (c) 2025 Signalsmith Audio）。`fft.h` と `stft.h` だけ。

crates.io の `signalsmith-stretch` 0.1.3（colinmarc/signalsmith-stretch-rs）に入っている版の写し。中身は直していない。
あの crate は bindgen（libclang）を要るので使わず、`src/wrapper.cpp` の小さな C の口だけを自前で書いた。
