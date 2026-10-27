//! Test instrumentation independent of loading progress and thread-local scopes.
//! Hooks are copied before invocation: a stopped I/O never holds the registry.
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type Hook = Arc<dyn Fn(&str, &Path) -> std::io::Result<()> + Send + Sync>;
static HOOKS: Mutex<Vec<(u64, Hook)>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
thread_local! { static UI_SCOPE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// Scope actual UI callbacks in gate tests. This detects every instrumented
/// operation, including new helpers outside the UI crate, independently of
/// which particular operation the test happens to hold.
pub struct UiScope(std::marker::PhantomData<std::rc::Rc<()>>);
pub fn forbid_ui_io() -> UiScope {
    UI_SCOPE.set(UI_SCOPE.get() + 1);
    UiScope(std::marker::PhantomData)
}
impl Drop for UiScope {
    fn drop(&mut self) {
        UI_SCOPE.set(UI_SCOPE.get() - 1);
    }
}

pub struct Observation(u64);
pub fn observe(hook: Hook) -> Observation {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    HOOKS.lock().unwrap().push((id, hook));
    Observation(id)
}
impl Drop for Observation {
    fn drop(&mut self) {
        HOOKS.lock().unwrap().retain(|(id, _)| *id != self.0);
    }
}
pub fn before(op: &str, path: &Path) -> std::io::Result<()> {
    assert!(
        UI_SCOPE.get() == 0
            || matches!(
                op,
                "writer_complete" | "recent_poll_empty" | "request_after_peek"
            ),
        "UI callback reached I/O boundary: {op}"
    );
    let hooks: Vec<_> = HOOKS
        .lock()
        .unwrap()
        .iter()
        .map(|(_, hook)| hook.clone())
        .collect();
    for hook in hooks {
        hook(op, path)?;
    }
    Ok(())
}
