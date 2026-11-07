//! Enqueue a real host query from inside the last block of each second, without sleeping.
use std::sync::{Arc, Mutex, Weak, mpsc};

use xirius_ara::fake::{self, Config};
use xirius_ara::service::{Binary, Instance, Loader, Probe, Stored};
use xirius_ara::{Error, Service, sys};

pub struct Checkpoints {
    pub replies: mpsc::Receiver<mpsc::Receiver<Probe>>,
    pub saves: mpsc::Receiver<Stored>,
    service: Arc<Mutex<Weak<Service>>>,
}

impl Checkpoints {
    pub fn new(save_on_first: bool) -> (Self, Loader) {
        let (tx, replies) = mpsc::channel();
        let (save_tx, saves) = mpsc::channel();
        let service = Arc::new(Mutex::new(Weak::new()));
        let route = service.clone();
        let loader: Loader = Box::new(move || {
            Ok(Box::new(CheckpointBinary {
                inner: fake::loader(Config::default())()?,
                service: route,
                tx,
                save_tx,
                save_on_first,
            }))
        });
        (
            Self {
                replies,
                saves,
                service,
            },
            loader,
        )
    }

    pub fn attach(&self, service: &Arc<Service>) {
        *self.service.lock().unwrap() = Arc::downgrade(service);
    }
}

struct CheckpointBinary {
    inner: Box<dyn Binary>,
    service: Arc<Mutex<Weak<Service>>>,
    tx: mpsc::Sender<mpsc::Receiver<Probe>>,
    save_tx: mpsc::Sender<Stored>,
    save_on_first: bool,
}

impl Binary for CheckpointBinary {
    fn factory(&self) -> *const sys::Factory {
        self.inner.factory()
    }

    fn instance(
        &mut self,
        dc: sys::Ref,
        known: sys::RoleFlags,
        assigned: sys::RoleFlags,
    ) -> Result<Box<dyn Instance>, Error> {
        Ok(Box::new(CheckpointInstance {
            inner: self.inner.instance(dc, known, assigned)?,
            service: self.service.clone(),
            tx: self.tx.clone(),
            save_tx: self.save_tx.clone(),
            save_on_first: self.save_on_first,
            frames: 0,
        }))
    }
}

struct CheckpointInstance {
    inner: Box<dyn Instance>,
    service: Arc<Mutex<Weak<Service>>>,
    tx: mpsc::Sender<mpsc::Receiver<Probe>>,
    save_tx: mpsc::Sender<Stored>,
    save_on_first: bool,
    frames: usize,
}

impl Instance for CheckpointInstance {
    fn extension(&self) -> sys::PlugInExtensionInstance {
        self.inner.extension()
    }

    fn prepare(&mut self, rate: f64, max_block: usize) -> Result<u32, Error> {
        self.inner.prepare(rate, max_block)
    }

    fn set_active(&mut self, active: bool) -> Result<(), Error> {
        self.inner.set_active(active)
    }

    fn set_transport(&mut self, playing: bool, position: i64) {
        self.inner.set_transport(playing, position);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.inner.process(left, right);
        let second = self.frames / 48_000;
        self.frames += left.len();
        if self.frames / 48_000 != second {
            let service = self.service.lock().unwrap().upgrade().unwrap();
            if second == 0 && self.save_on_first {
                self.save_tx
                    .send(service.store_changed_later().unwrap())
                    .unwrap();
            }
            self.tx.send(service.probe_later().unwrap()).unwrap();
        }
    }
}
