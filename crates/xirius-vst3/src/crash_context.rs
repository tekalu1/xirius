//! Same-thread crash breadcrumbs with no allocation or locks on entry or in the filter.
use std::cell::Cell;
use std::marker::PhantomData;
#[derive(Clone, Copy)]
struct Context {
    bytes: [u8; 1024],
    len: usize,
}
const EMPTY: Context = Context {
    bytes: [0; 1024],
    len: 0,
};
thread_local! { static ACTIVE: Cell<Context> = const { Cell::new(EMPTY) }; }
/// Restores the enclosing call's marker. A guard stays on its calling thread.
pub struct Guard {
    previous: Context,
    _thread: PhantomData<*mut ()>,
}
pub fn enter(text: &str) -> Guard {
    let mut next = EMPTY;
    if text.len() <= next.bytes.len() {
        next.bytes[..text.len()].copy_from_slice(text.as_bytes());
        next.len = text.len();
    }
    Guard {
        previous: ACTIVE.with(|p| p.replace(next)),
        _thread: PhantomData,
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        ACTIVE.with(|p| p.set(self.previous));
    }
}
/// Called synchronously on the faulting thread; the fixed buffer contains complete UTF-8.
pub fn with_active(f: impl FnOnce(&str)) {
    let _ = ACTIVE.try_with(|p| {
        let context = p.get();
        if let Ok(text) = std::str::from_utf8(&context.bytes[..context.len]) {
            f(text);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_calls_restore_and_other_threads_have_no_marker() {
        let outer = enter("vst3:outer");
        {
            let _inner = enter("clap:inner");
            with_active(|s| assert_eq!(s, "clap:inner"));
        }
        with_active(|s| assert_eq!(s, "vst3:outer"));
        std::thread::spawn(|| with_active(|s| assert!(s.is_empty())))
            .join()
            .unwrap();
        let _ = std::panic::catch_unwind(|| {
            let _scope = enter("vst3:unwinding");
            panic!("fixture unwind");
        });
        with_active(|s| assert_eq!(s, "vst3:outer"));
        {
            let _oversized = enter(&"x".repeat(2048));
            with_active(|s| assert!(s.is_empty()));
        }
        drop(outer);
        with_active(|s| assert!(s.is_empty()));
    }
}
