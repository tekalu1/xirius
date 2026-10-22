//! Engine locks assert before acquiring when called from an audio callback.
use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::{LockResult, MutexGuard, TryLockResult};

thread_local! { static AUDIO: Cell<bool> = const { Cell::new(false) }; }

pub struct AudioScope {
    previous: bool,
    _thread: PhantomData<*mut ()>,
}
impl AudioScope {
    pub fn enter() -> Self {
        Self {
            previous: AUDIO.replace(true),
            _thread: PhantomData,
        }
    }
}
impl Drop for AudioScope {
    fn drop(&mut self) {
        AUDIO.set(self.previous);
    }
}

#[derive(Debug, Default)]
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self(std::sync::Mutex::new(value))
    }
    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        assert_can_lock();
        self.0.lock()
    }
    pub fn try_lock(&self) -> TryLockResult<MutexGuard<'_, T>> {
        assert_can_lock();
        self.0.try_lock()
    }
    pub fn get_mut(&mut self) -> LockResult<&mut T> {
        self.0.get_mut()
    }
    pub fn into_inner(self) -> LockResult<T> {
        self.0.into_inner()
    }
}

fn assert_can_lock() {
    assert!(!AUDIO.get(), "audio callback attempted to acquire a lock");
}
