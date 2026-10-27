//! Device callbacks belong to this worker, including construction and Drop.
//! Shared locks contain value reports only, never a device or an I/O call.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

type Config = (String, String, u32);
#[derive(Clone, Default)]
struct Report {
    generation: u64,
    devices: Vec<Device>,
    latency: Option<Latency>,
    levels: Vec<f32>,
    measure: bool,
    error: Option<String>,
}
#[derive(Clone)]
pub(super) struct Proxy {
    last: std::cell::RefCell<Report>,
    tx: mpsc::Sender<(u64, Config)>,
    wanted: Arc<Mutex<Option<Config>>>,
    generation: Arc<AtomicU64>,
    report: Arc<Mutex<Report>>,
}
impl Proxy {
    pub(super) fn new(
        make: impl FnOnce() -> Box<dyn Backend> + Send + 'static,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<(u64, Config)>();
        let report = Arc::new(Mutex::new(Report::default()));
        let result = report.clone();
        let generation = Arc::new(AtomicU64::new(0));
        let current = generation.clone();
        let _owner = crate::background::Job::spawn("device-reports", move || {
            let mut backend = make();
            let mut configured = None;
            let mut devices = Vec::new();
            let mut latency = None;
            let mut error = None;
            let mut version = 0;
            let mut needs_config = false;
            let mut inventory_at = Instant::now();
            loop {
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(mut message) => {
                        loop {
                            match rx.try_recv() {
                                Ok(next) => message = next,
                                Err(mpsc::TryRecvError::Empty) => break,
                                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                            }
                        }
                        (version, configured) = (message.0, Some(message.1));
                        needs_config = true;
                        inventory_at = Instant::now();
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if Instant::now() >= inventory_at {
                    devices = backend.devices();
                    inventory_at = Instant::now() + Duration::from_secs(1);
                }
                if needs_config {
                    needs_config = false;
                    if let Some((driver, device, buffer)) = &configured {
                        let device = if device.is_empty() {
                            automatic(&devices)
                        } else {
                            device.clone()
                        };
                        match backend.configure(driver, &device, *buffer) {
                            Ok(value) => {
                                latency = value;
                                error = None;
                            }
                            Err(value) => error = Some(value),
                        }
                    }
                }
                let measure = backend.can_measure();
                let levels = if measure {
                    backend.levels()
                } else {
                    Vec::new()
                };
                if current.load(Ordering::Acquire) == version {
                    *result.lock().unwrap_or_else(|e| e.into_inner()) = Report {
                        generation: version,
                        devices: devices.clone(),
                        latency,
                        levels,
                        measure,
                        error: error.clone(),
                    };
                }
            }
        })?;
        Ok(Self {
            last: Default::default(),
            tx,
            wanted: Default::default(),
            generation,
            report,
        })
    }
    fn cached<T>(&self, read: impl FnOnce(&Report) -> T) -> T {
        if let Ok(report) = self.report.try_lock() {
            *self.last.borrow_mut() = report.clone();
        }
        read(&self.last.borrow())
    }
}
impl Backend for Proxy {
    fn can_measure(&self) -> bool {
        self.cached(|r| r.measure)
    }
    fn devices(&self) -> Vec<Device> {
        self.cached(|r| r.devices.clone())
    }
    fn configure(
        &mut self,
        driver: &str,
        device: &str,
        buffer: u32,
    ) -> Result<Option<Latency>, String> {
        let config = (driver.into(), device.into(), buffer);
        if let Ok(mut wanted) = self.wanted.try_lock()
            && wanted.as_ref() != Some(&config)
        {
            let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
            self.tx
                .send((generation, config.clone()))
                .map_err(|_| "装置の監視が停止しました".to_string())?;
            *wanted = Some(config);
        }
        self.cached(|report| {
            if report.generation != self.generation.load(Ordering::Acquire) {
                return Ok(None);
            }
            match &report.error {
                Some(error) => Err(error.clone()),
                None => Ok(report.latency),
            }
        })
    }
    fn levels(&mut self) -> Vec<f32> {
        self.cached(|r| r.levels.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Condvar;

    struct Held {
        gate: Arc<(Mutex<bool>, Condvar)>,
        entered: mpsc::Sender<std::thread::ThreadId>,
        ui: std::thread::ThreadId,
        dropped: mpsc::Sender<()>,
    }
    impl Backend for Held {
        fn devices(&self) -> Vec<Device> {
            Vec::new()
        }
        fn configure(&mut self, _: &str, _: &str, buffer: u32) -> Result<Option<Latency>, String> {
            assert_ne!(std::thread::current().id(), self.ui);
            let _ = self.entered.send(std::thread::current().id());
            drop(
                self.gate
                    .1
                    .wait_while(self.gate.0.lock().unwrap(), |done| !*done)
                    .unwrap(),
            );
            Ok(Some(Latency {
                rate: 48000,
                input: buffer,
                output: 0,
                conversion: 0,
            }))
        }
        fn levels(&mut self) -> Vec<f32> {
            Vec::new()
        }
    }
    impl Drop for Held {
        fn drop(&mut self) {
            assert_ne!(std::thread::current().id(), self.ui);
            let _ = self.dropped.send(());
        }
    }
    #[test]
    fn held_device_configuration_never_blocks_reports_supersession_or_drop() {
        let ui = std::thread::current().id();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(gate.clone());
        let (entered, rx) = mpsc::channel();
        let (dropped, end) = mpsc::channel();
        let mut proxy = Proxy::new(move || {
            Box::new(Held {
                gate,
                entered,
                dropped,
                ui,
            })
        })
        .unwrap();
        proxy.configure("", "fixture", 64).unwrap();
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        for _ in 0..32 {
            assert!(proxy.devices().is_empty());
            assert!(proxy.levels().is_empty());
            assert_eq!(proxy.configure("", "fixture", 128).unwrap(), None);
        }
        assert!(proxy.report.try_lock().is_ok());
        drop(proxy);
        drop(release);
        end.recv_timeout(Duration::from_secs(20)).unwrap();
    }
}
