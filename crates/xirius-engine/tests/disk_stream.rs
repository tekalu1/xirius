//! A large file streams through bounded memory; sequential playback and seeks stay exact.
use assert_no_alloc::{AllocDisabler, reset_violation_count, violation_count};
use xirius_engine::disk_stream::DiskAudio;
#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

fn sample(frame: usize) -> f32 {
    (frame % 8191) as f32 / 16384.0
}

#[test]
fn wav_samples_are_read_on_the_worker_and_played_without_audio_thread_io_or_allocation() {
    let dir = tempfile::Builder::new()
        .prefix("xirius-stream-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("stream.wav");
    let frames = 524_288;
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for i in 0..frames {
        writer.write_sample(sample(i)).unwrap();
        writer.write_sample(-sample(i)).unwrap();
    }
    writer.finalize().unwrap();
    let disk = DiskAudio::open(&path, 48_000).unwrap().unwrap();
    assert_eq!(disk.frames(), frames as u64);
    disk.prime(0);
    reset_violation_count();
    for at in (0..frames).step_by(997) {
        let n = 997.min(frames - at);
        let (mut l, mut r) = ([0.0; 997], [0.0; 997]);
        assert_no_alloc::assert_no_alloc(|| {
            disk.mix(at as u64, n, true, |i, a, b| {
                l[i] = a;
                r[i] = b;
            })
        });
        for i in 0..n {
            assert_eq!(l[i], sample(at + i));
            assert_eq!(r[i], -sample(at + i));
        }
    }
    // A backwards jump discards stale ring pages; a jump near EOF stays clipped to the file.
    for at in [123_456, 0, frames - 37] {
        let n = 37.min(frames - at);
        disk.mix(at as u64, n, true, |i, l, r| {
            assert_eq!(l, sample(at + i));
            assert_eq!(r, -l);
        });
    }
    assert_eq!(violation_count(), 0);
    assert!(
        disk.disk_reads() >= 64,
        "the worker refilled the fixed ring while playing"
    );
    assert_eq!(disk.missed_frames(), 0);
    assert!(!disk.failed());
    // A file truncated after playback began is an observable reader failure.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(256)
        .unwrap();
    disk.mix(123_456, 64, true, |_, _, _| {});
    assert!(disk.failed());
    drop(disk);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn resident_and_streamed_clips_keep_the_original_summation_order() {
    use std::sync::Arc;
    use xirius_engine::timeline::StreamRegion;
    use xirius_engine::{AudioData, Region, Timeline};
    let dir = tempfile::Builder::new()
        .prefix("xirius-stream-order-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("order.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for _ in 0..64 {
        writer.write_sample(1e20f32).unwrap();
    }
    writer.finalize().unwrap();
    let region = |value| Region {
        start: 0,
        len: 64,
        offset: 0,
        gain: 1.0,
        data: Arc::new(AudioData {
            left: vec![value; 64],
            right: vec![value; 64],
        }),
        shape: Default::default(),
    };
    let reference = Timeline::new(Vec::new(), vec![region(1e20), region(-1e20), region(1.0)]);
    let mut streamed = Timeline::new(Vec::new(), vec![region(-1e20), region(1.0)]);
    let data = DiskAudio::open(&path, 48_000).unwrap().unwrap();
    data.prime(0);
    streamed.push_stream_at(
        0,
        StreamRegion {
            start: 0,
            len: 64,
            offset: 0,
            gain: 1.0,
            data,
            shape: Default::default(),
        },
    );
    streamed.set_offline(true);
    let (mut a, mut b, mut c, mut d) = ([0.0; 64], [0.0; 64], [0.0; 64], [0.0; 64]);
    reference.mix_audio(0, &mut a, &mut b);
    streamed.mix_audio(0, &mut c, &mut d);
    assert_eq!(a, [1.0; 64]);
    assert_eq!(a, c);
    assert_eq!(b, d);
    drop(streamed);
    std::fs::remove_file(path).unwrap();
}
