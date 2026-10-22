// Signalsmith Stretch を、ファイル全体を一度に伸縮する C の口にする。
// 中身は SignalsmithStretch::exact と同じ手順で、process だけを塊に分けて、途中で進み具合を返す（止められる）。
#include "signalsmith-stretch/signalsmith-stretch.h"

#include <algorithm>
#include <cstddef>

namespace {

// 平面の入力。範囲の外は 0 を返す（頭と尻の詰め物）。
struct Input {
    const float *const *channels;
    long long offset;
    long long length;

    struct Channel {
        const Input &input;
        int channel;
        float operator[](int i) const {
            long long j = input.offset + i;
            return (j >= 0 && j < input.length) ? input.channels[channel][j] : 0.0f;
        }
    };

    Channel operator[](int c) const { return {*this, c}; }
};

// 平面の出力を、offset から先として見せる。
struct Output {
    float *const *channels;
    long long offset;

    struct Channel {
        const Output &output;
        int channel;
        float &operator[](int i) const { return output.channels[channel][output.offset + i]; }
    };

    Channel operator[](int c) const { return {*this, c}; }
};

using Stretch = signalsmith::stretch::SignalsmithStretch<float>;

const int CHUNK = 8192;

}  // namespace

extern "C" {

// 出力がこれより短いと exact の手順が使えない（呼ぶ側で 0 を詰めて伸ばす）。
int xirius_stretch_min_output(int channels, float rate) {
    Stretch s;
    s.presetDefault(channels, rate);
    return s.outputLatency() * 2;
}

// 0: できた。1: 止められた。2: 出力が短すぎる。
int xirius_stretch_render(int channels, float rate, const float *const *input, size_t input_length,
                          float *const *output, size_t output_length, float transpose,
                          int (*progress)(void *, double), void *user) {
    Stretch s;
    s.presetDefault(channels, rate);
    if (transpose != 1.0f) s.setTransposeFactor(transpose);

    const long long in_n = (long long)input_length;
    const long long out_n = (long long)output_length;
    const int in_lat = s.inputLatency();
    const int out_lat = s.outputLatency();
    if (out_n < (long long)out_lat * 2) return 2;

    // 入力の真ん中に置いて始める（exact と同じ）。
    Input head{input, 0, in_n};
    s.seek(head, in_lat, double(in_n) / double(out_n));

    long long in_done = 0;
    for (long long out_done = 0; out_done < out_n;) {
        long long out_end = std::min(out_n, out_done + CHUNK);
        long long in_end = out_end * in_n / out_n;
        Input view{input, in_lat + in_done, in_n};
        Output out{output, out_done};
        s.process(view, int(in_end - in_done), out, int(out_end - out_done));
        in_done = in_end;
        out_done = out_end;
        if (progress && progress(user, double(out_done) / double(out_n))) return 1;
    }

    // 頭の折り返しと、出力の遅れの分の詰め直し（exact と同じ）。
    for (int c = 0; c < channels; ++c) {
        float *ch = output[c];
        long long fold = std::min<long long>(out_n - out_lat, out_lat);
        for (long long i = 0; i < fold; ++i) ch[i + out_lat] -= ch[out_lat - 1 - i];
        for (long long i = 0; i < out_n - out_lat; ++i) ch[i] = ch[i + out_lat];
    }
    Output tail{output, out_n - out_lat};
    s.flush(tail, out_lat);
    return 0;
}

}  // extern "C"
