use super::*;
use std::sync::Mutex;
use std::time::Instant;

struct Held {
    audio: AudioEngine,
    stopped: mpsc::Sender<std::thread::ThreadId>,
    release: mpsc::Receiver<()>,
}

impl SharedStream for Held {
    fn name(&self) -> String {
        "fixture".into()
    }
    fn lost(&self) -> Lost {
        Lost::default()
    }
    fn stop(self: Box<Self>) -> Option<AudioEngine> {
        self.stopped.send(std::thread::current().id()).unwrap();
        self.release.recv().unwrap();
        Some(self.audio)
    }
}

fn audio() -> AudioEngine {
    let setup = crate::build::setup(48000, xirius_engine::RenderMode::Realtime);
    let plan = xirius_engine::Plan::build(Default::default(), setup, None).unwrap();
    xirius_engine::engine::create(plan, Default::default()).0
}

fn wait<T: Send + 'static>(job: &crate::background::Job<T>) -> Result<T, String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "output watchdog");
        std::thread::yield_now();
    }
}

#[test]
fn cancellation_during_open_keeps_output_stop_off_the_caller_and_waits_for_cleanup_ack() {
    let ui = std::thread::current().id();
    let (entered, opening) = mpsc::channel();
    let (go, gate) = mpsc::channel();
    let (stopped, stopping) = mpsc::channel();
    let (release, retiring) = mpsc::channel();
    let gate = Mutex::new(Some((gate, retiring)));
    let mut drivers = Drivers::system();
    drivers.exclusive_out = None;
    drivers.devices = Arc::new(Vec::new);
    drivers.shared_out = Arc::new(move |audio, _, _| {
        let (gate, retiring) = gate.lock().unwrap().take().unwrap();
        let entered = entered.clone();
        let stopped = stopped.clone();
        DeviceOutput::open_with(audio, move |audio| {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            Ok(Box::new(Held {
                audio,
                stopped,
                release: retiring,
            }))
        })
        .map(Output::Device)
    });
    let job =
        Output::open_background(OutputMode::Device, audio(), Default::default(), drivers).unwrap();
    opening.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(job.poll().is_none());
    job.cancel();
    assert!(!job.finished());
    go.send(()).unwrap();
    assert_ne!(stopping.recv_timeout(Duration::from_secs(10)).unwrap(), ui);
    assert!(!job.finished(), "stop is still held at the I/O gate");
    assert!(
        job.poll().is_none(),
        "cancelled success must never be adopted"
    );
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !job.finished() {
        assert!(Instant::now() < deadline, "cleanup watchdog");
        std::thread::yield_now();
    }
}

#[test]
fn retiring_an_adopted_output_accepts_immediately_and_acknowledges_only_after_stop() {
    let ui = std::thread::current().id();
    let (stopped, stopping) = mpsc::channel();
    let (release, retiring) = mpsc::channel();
    let output = DeviceOutput::open_with(audio(), move |audio| {
        Ok(Box::new(Held {
            audio,
            stopped,
            release: retiring,
        }))
    })
    .unwrap();
    let job = Output::Device(output).retire().unwrap();
    assert_ne!(stopping.recv_timeout(Duration::from_secs(10)).unwrap(), ui);
    assert!(job.poll().is_none());
    assert!(!job.finished());
    release.send(()).unwrap();
    wait(&job).unwrap();
}
