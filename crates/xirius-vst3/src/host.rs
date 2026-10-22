//! Objects the host hands to the plugin: the host context, messages between the processor and
//! the controller, and the component handler that receives edits from the plugin's editor.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use vst3::Steinberg::Vst::IAttributeList_::AttrID;
use vst3::Steinberg::Vst::{
    IAttributeList, IAttributeListTrait, IComponentHandler, IComponentHandlerTrait,
    IHostApplication, IHostApplicationTrait, IMessage, IMessageTrait, ParamID, ParamValue,
    String128, TChar,
};
use vst3::Steinberg::{
    FIDString, TUID, int32, int64, kInvalidArgument, kNoInterface, kResultFalse, kResultOk,
    tresult, uint32,
};
use vst3::{Class, ComWrapper, Interface};

/// The host context passed to `initialize`. Plugins ask it for their message objects.
pub struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

fn guid(p: *const TUID) -> [u8; 16] {
    // SAFETY: callers check for null; a TUID is 16 bytes.
    unsafe { (*p).map(|c| c as u8) }
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        if name.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the plugin passes a String128 it owns.
        let dst = unsafe { &mut *name };
        dst.fill(0);
        for (d, c) in dst.iter_mut().zip("Xirius".encode_utf16()) {
            *d = c;
        }
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        let (cid, iid) = (guid(cid), guid(iid));
        let made = if cid == IMessage::IID && iid == IMessage::IID {
            ComWrapper::new(HostMessage::default())
                .to_com_ptr::<IMessage>()
                .map(|p| p.into_raw() as *mut c_void)
        } else if cid == IAttributeList::IID && iid == IAttributeList::IID {
            ComWrapper::new(HostAttributeList::default())
                .to_com_ptr::<IAttributeList>()
                .map(|p| p.into_raw() as *mut c_void)
        } else {
            None
        };
        // SAFETY: `obj` was checked for null.
        unsafe { *obj = made.unwrap_or(std::ptr::null_mut()) };
        if made.is_some() {
            kResultOk
        } else {
            kNoInterface
        }
    }
}

enum Attr {
    Int(i64),
    Float(f64),
    /// NUL-terminated.
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

/// Key-value storage attached to a message.
#[derive(Default)]
pub struct HostAttributeList {
    values: Mutex<HashMap<CString, Attr>>,
}

impl Class for HostAttributeList {
    type Interfaces = (IAttributeList,);
}

fn key(id: AttrID) -> Option<CString> {
    // SAFETY: attribute ids are NUL-terminated strings from the plugin.
    (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned())
}

impl HostAttributeList {
    fn set(&self, id: AttrID, value: Attr) -> tresult {
        let Some(k) = key(id) else {
            return kInvalidArgument;
        };
        self.values.lock().unwrap().insert(k, value);
        kResultOk
    }
}

impl IAttributeListTrait for HostAttributeList {
    unsafe fn setInt(&self, id: AttrID, value: int64) -> tresult {
        self.set(id, Attr::Int(value))
    }

    unsafe fn getInt(&self, id: AttrID, value: *mut int64) -> tresult {
        let (Some(k), false) = (key(id), value.is_null()) else {
            return kInvalidArgument;
        };
        match self.values.lock().unwrap().get(&k) {
            Some(Attr::Int(v)) => {
                // SAFETY: checked for null above.
                unsafe { *value = *v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        self.set(id, Attr::Float(value))
    }

    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        let (Some(k), false) = (key(id), value.is_null()) else {
            return kInvalidArgument;
        };
        match self.values.lock().unwrap().get(&k) {
            Some(Attr::Float(v)) => {
                // SAFETY: checked for null above.
                unsafe { *value = *v };
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut s = Vec::new();
        // SAFETY: the plugin passes a NUL-terminated UTF-16 string.
        unsafe {
            let mut p = string;
            while *p != 0 {
                s.push(*p);
                p = p.add(1);
            }
        }
        s.push(0);
        self.set(id, Attr::Str(s))
    }

    unsafe fn getString(&self, id: AttrID, string: *mut TChar, size_in_bytes: uint32) -> tresult {
        let (Some(k), false) = (key(id), string.is_null()) else {
            return kInvalidArgument;
        };
        let cap = size_in_bytes as usize / 2;
        match self.values.lock().unwrap().get(&k) {
            Some(Attr::Str(s)) if cap > 0 => {
                let n = s.len().min(cap);
                // SAFETY: the plugin's buffer holds `cap` chars; the copy is at most that and
                // the last char written is NUL.
                unsafe {
                    std::ptr::copy_nonoverlapping(s.as_ptr(), string, n);
                    *string.add(n - 1) = 0;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }

    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size_in_bytes: uint32) -> tresult {
        if data.is_null() && size_in_bytes > 0 {
            return kInvalidArgument;
        }
        let bytes = if size_in_bytes == 0 {
            Vec::new()
        } else {
            // SAFETY: the plugin passes `size_in_bytes` readable bytes.
            unsafe { std::slice::from_raw_parts(data as *const u8, size_in_bytes as usize) }
                .to_vec()
        };
        self.set(id, Attr::Bin(bytes))
    }

    unsafe fn getBinary(
        &self,
        id: AttrID,
        data: *mut *const c_void,
        size_in_bytes: *mut uint32,
    ) -> tresult {
        let (Some(k), false, false) = (key(id), data.is_null(), size_in_bytes.is_null()) else {
            return kInvalidArgument;
        };
        match self.values.lock().unwrap().get(&k) {
            Some(Attr::Bin(b)) => {
                // SAFETY: out-pointers checked above. The bytes stay put until the attribute
                // is replaced or the list is released, which is the SDK's contract too.
                unsafe {
                    *data = b.as_ptr() as *const c_void;
                    *size_in_bytes = b.len() as uint32;
                }
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

/// A message sent between a plugin's processor and its controller through the host.
pub struct HostMessage {
    id: Mutex<Option<CString>>,
    attributes: ComWrapper<HostAttributeList>,
}

impl Default for HostMessage {
    fn default() -> Self {
        Self {
            id: Mutex::new(None),
            attributes: ComWrapper::new(HostAttributeList::default()),
        }
    }
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> FIDString {
        match &*self.id.lock().unwrap() {
            // The CString's heap buffer lives until the id is replaced.
            Some(s) => s.as_ptr(),
            None => std::ptr::null(),
        }
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        *self.id.lock().unwrap() = key(id);
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // Not add-ref'd, as in the SDK's own host message.
        self.attributes
            .as_com_ref::<IAttributeList>()
            .map(|r| r.as_ptr())
            .unwrap_or(std::ptr::null_mut())
    }
}

/// One step of a user's edit in the plugin's editor, in the order the editor reported it: a
/// knob grabbed, moved and let go. Automation writing (Touch / Latch) follows these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    Begin(ParamID),
    Perform(ParamID, ParamValue),
    End(ParamID),
}

/// How many gestures are kept until [`ComponentHandler::take_gestures`] collects them. The
/// oldest are dropped past this.
const MAX_GESTURES: usize = 4096;

/// Receives parameter edits from the plugin's editor and forwards them to the processor
/// through a wait-free queue. It also records them as [`Gesture`]s for the UI thread.
pub struct ComponentHandler {
    edits: Mutex<rtrb::Producer<(ParamID, ParamValue)>>,
    gestures: Mutex<Vec<Gesture>>,
    restart: AtomicI32,
    edits_seen: AtomicU64,
    edits_dropped: AtomicU64,
}

impl Class for ComponentHandler {
    type Interfaces = (IComponentHandler,);
}

impl ComponentHandler {
    pub(crate) fn new(edits: rtrb::Producer<(ParamID, ParamValue)>) -> Self {
        Self {
            edits: Mutex::new(edits),
            gestures: Mutex::new(Vec::new()),
            restart: AtomicI32::new(0),
            edits_seen: AtomicU64::new(0),
            edits_dropped: AtomicU64::new(0),
        }
    }

    /// Queues a parameter change for the processor. Used for the editor's edits and for
    /// changes made by the host.
    pub fn push_edit(&self, id: ParamID, value: ParamValue) -> bool {
        self.edits_seen.fetch_add(1, Ordering::Relaxed);
        let ok = self.edits.lock().unwrap().push((id, value)).is_ok();
        if !ok {
            self.edits_dropped.fetch_add(1, Ordering::Relaxed);
        }
        ok
    }

    fn record(&self, g: Gesture) {
        let mut list = self.gestures.lock().unwrap();
        if list.len() >= MAX_GESTURES {
            list.remove(0);
        }
        list.push(g);
    }

    /// The editor's gestures since the last call, oldest first.
    pub fn take_gestures(&self) -> Vec<Gesture> {
        std::mem::take(&mut *self.gestures.lock().unwrap())
    }

    /// `restartComponent` flags asked for since the last call (latency, I/O, reload).
    pub fn take_restart_flags(&self) -> i32 {
        self.restart.swap(0, Ordering::AcqRel)
    }

    /// (edits received, edits dropped because the queue was full).
    pub fn edit_counts(&self) -> (u64, u64) {
        (
            self.edits_seen.load(Ordering::Relaxed),
            self.edits_dropped.load(Ordering::Relaxed),
        )
    }
}

impl IComponentHandlerTrait for ComponentHandler {
    unsafe fn beginEdit(&self, id: ParamID) -> tresult {
        self.record(Gesture::Begin(id));
        kResultOk
    }

    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        self.record(Gesture::Perform(id, value));
        if self.push_edit(id, value) {
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn endEdit(&self, id: ParamID) -> tresult {
        self.record(Gesture::End(id));
        kResultOk
    }

    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.restart.fetch_or(flags, Ordering::AcqRel);
        kResultOk
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vst3::ComPtr;

    #[test]
    fn host_creates_messages_with_attributes() {
        let host = ComWrapper::new(HostApplication);
        let mut cid = IMessage::IID.map(|b| b as std::ffi::c_char);
        let mut iid = cid;
        let mut obj = std::ptr::null_mut();
        let r = unsafe { host.createInstance(&mut cid, &mut iid, &mut obj) };
        assert_eq!(r, kResultOk);
        let msg = unsafe { ComPtr::<IMessage>::from_raw(obj as *mut IMessage) }.unwrap();
        unsafe { msg.setMessageID(c"hello".as_ptr()) };
        let id = unsafe { CStr::from_ptr(msg.getMessageID()) };
        assert_eq!(id.to_str().unwrap(), "hello");
        let attrs = unsafe { msg.getAttributes() };
        let attrs = unsafe { vst3::ComRef::from_raw(attrs) }.unwrap();
        unsafe { attrs.setInt(c"n".as_ptr(), 42) };
        let mut n = 0;
        assert_eq!(unsafe { attrs.getInt(c"n".as_ptr(), &mut n) }, kResultOk);
        assert_eq!(n, 42);
        let blob = [1u8, 2, 3];
        unsafe { attrs.setBinary(c"b".as_ptr(), blob.as_ptr().cast(), 3) };
        let (mut p, mut len) = (std::ptr::null(), 0);
        assert_eq!(
            unsafe { attrs.getBinary(c"b".as_ptr(), &mut p, &mut len) },
            kResultOk
        );
        assert_eq!(
            unsafe { std::slice::from_raw_parts(p as *const u8, len as usize) },
            &blob
        );
        let text: Vec<u16> = "ab".encode_utf16().chain([0]).collect();
        unsafe { attrs.setString(c"s".as_ptr(), text.as_ptr()) };
        let mut out = [0u16; 8];
        unsafe { attrs.getString(c"s".as_ptr(), out.as_mut_ptr(), 16) };
        assert_eq!(&out[..3], &text[..]);
        // Unknown classes are refused.
        let mut other = [7 as std::ffi::c_char; 16];
        let r = unsafe { host.createInstance(&mut other, &mut other.clone(), &mut obj) };
        assert_eq!(r, kNoInterface);
    }

    #[test]
    fn handler_forwards_edits_until_the_queue_is_full() {
        let (tx, mut rx) = rtrb::RingBuffer::new(2);
        let h = ComWrapper::new(ComponentHandler::new(tx));
        unsafe {
            assert_eq!(h.performEdit(1, 0.5), kResultOk);
            assert_eq!(h.performEdit(2, 0.25), kResultOk);
            assert_eq!(h.performEdit(3, 1.0), kResultFalse);
            h.restartComponent(8);
        }
        assert_eq!(rx.pop().unwrap(), (1, 0.5));
        assert_eq!(rx.pop().unwrap(), (2, 0.25));
        assert_eq!(h.edit_counts(), (3, 1));
        assert_eq!(h.take_restart_flags(), 8);
        assert_eq!(h.take_restart_flags(), 0);
    }

    #[test]
    fn handler_records_the_editor_gestures_in_order() {
        let (tx, _rx) = rtrb::RingBuffer::new(8);
        let h = ComWrapper::new(ComponentHandler::new(tx));
        unsafe {
            h.beginEdit(4);
            h.performEdit(4, 0.25);
            h.performEdit(4, 0.5);
            h.endEdit(4);
        }
        assert_eq!(
            h.take_gestures(),
            vec![
                Gesture::Begin(4),
                Gesture::Perform(4, 0.25),
                Gesture::Perform(4, 0.5),
                Gesture::End(4)
            ]
        );
        assert!(h.take_gestures().is_empty());
        // Host changes through push_edit are not the user's gestures.
        h.push_edit(4, 1.0);
        assert!(h.take_gestures().is_empty());
    }
}
