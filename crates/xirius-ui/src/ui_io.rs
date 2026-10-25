//! Runtime work is not part of a browser's visible value. Cloning a browser
//! starts an independent client; comparing it compares only its visible state.
pub(crate) struct UiIo<T>(T);
impl<T: Default> Default for UiIo<T> {
    fn default() -> Self {
        Self(T::default())
    }
}
impl<T: Default> Clone for UiIo<T> {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl<T> PartialEq for UiIo<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl<T> std::fmt::Debug for UiIo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UiIo")
    }
}
impl<T> std::ops::Deref for UiIo<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T> std::ops::DerefMut for UiIo<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
