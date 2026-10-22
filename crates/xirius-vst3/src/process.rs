//! Event lists and parameter changes for `process`, preallocated so the audio thread only
//! clears and refills them.
//!
//! The COM methods take `&self`, so the storage sits in `UnsafeCell`s. Each object is owned
//! by one `AudioPart` and touched only by the thread calling its `process`, one call at a time.

use std::cell::{Cell, UnsafeCell};

use vst3::Steinberg::Vst::{
    Event, IEventList, IEventListTrait, IParamValueQueue, IParamValueQueueTrait, IParameterChanges,
    IParameterChangesTrait, ParamID, ParamValue,
};
use vst3::Steinberg::{int32, kInvalidArgument, kResultFalse, kResultOk, tresult};
use vst3::{Class, ComWrapper};

pub struct EventList {
    events: UnsafeCell<Vec<Event>>,
    capacity: usize,
    dropped: Cell<u64>,
}

// SAFETY: see the module comment; one thread at a time, never concurrently.
unsafe impl Send for EventList {}
unsafe impl Sync for EventList {}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl EventList {
    pub fn with_capacity(capacity: usize) -> ComWrapper<EventList> {
        ComWrapper::new(EventList {
            events: UnsafeCell::new(Vec::with_capacity(capacity)),
            capacity,
            dropped: Cell::new(0),
        })
    }

    #[allow(clippy::mut_from_ref)]
    fn events(&self) -> &mut Vec<Event> {
        // SAFETY: see the module comment. No reference escapes a single method call.
        unsafe { &mut *self.events.get() }
    }

    pub fn clear(&self) {
        self.events().clear();
    }

    /// Adds an event if there is room; full lists drop and count it.
    pub fn push(&self, e: Event) -> bool {
        let ev = self.events();
        if ev.len() < self.capacity {
            ev.push(e);
            true
        } else {
            self.dropped.set(self.dropped.get() + 1);
            false
        }
    }

    pub fn len(&self) -> usize {
        self.events().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.get()
    }
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.len() as int32
    }

    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        if e.is_null() {
            return kInvalidArgument;
        }
        match self.events().get(index as usize) {
            Some(ev) => {
                // SAFETY: checked for null above.
                unsafe { *e = *ev };
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        if e.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked for null above.
        if self.push(unsafe { *e }) {
            kResultOk
        } else {
            kResultFalse
        }
    }
}

/// The points of one parameter within one block.
pub struct ParamQueue {
    id: Cell<ParamID>,
    points: UnsafeCell<Vec<(int32, ParamValue)>>,
    capacity: usize,
}

// SAFETY: as for `EventList`.
unsafe impl Send for ParamQueue {}
unsafe impl Sync for ParamQueue {}

impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}

impl ParamQueue {
    #[allow(clippy::mut_from_ref)]
    fn points(&self) -> &mut Vec<(int32, ParamValue)> {
        // SAFETY: as for `EventList::events`.
        unsafe { &mut *self.points.get() }
    }

    /// Inserts a point in time order; a point at the same offset replaces it.
    fn add(&self, offset: int32, value: ParamValue) -> Option<usize> {
        let p = self.points();
        let i = p.partition_point(|point| point.0 < offset);
        if let Some(point) = p.get_mut(i)
            && point.0 == offset
        {
            point.1 = value;
            return Some(i);
        }
        if p.len() < self.capacity {
            p.insert(i, (offset, value));
            return Some(i);
        }
        None
    }

    pub fn last_value(&self) -> Option<ParamValue> {
        self.points().last().map(|p| p.1)
    }
}

impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }

    unsafe fn getPointCount(&self) -> int32 {
        self.points().len() as int32
    }

    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        if offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        match self.points().get(index as usize) {
            Some(&(o, v)) => {
                // SAFETY: checked for null above.
                unsafe {
                    *offset = o;
                    *value = v;
                }
                kResultOk
            }
            None => kInvalidArgument,
        }
    }

    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        match self.add(offset, value) {
            Some(i) => {
                if !index.is_null() {
                    // SAFETY: optional out-pointer, checked for null.
                    unsafe { *index = i as int32 };
                }
                kResultOk
            }
            None => kResultFalse,
        }
    }
}

/// The parameter changes of one block: a fixed pool of queues, of which the first `used` are
/// live.
pub struct ParameterChanges {
    queues: Vec<ComWrapper<ParamQueue>>,
    used: Cell<usize>,
    dropped: Cell<u64>,
}

// SAFETY: as for `EventList`.
unsafe impl Send for ParameterChanges {}
unsafe impl Sync for ParameterChanges {}

impl Class for ParameterChanges {
    type Interfaces = (IParameterChanges,);
}

impl ParameterChanges {
    pub fn with_capacity(params: usize, points: usize) -> ComWrapper<ParameterChanges> {
        let queues = (0..params)
            .map(|_| {
                ComWrapper::new(ParamQueue {
                    id: Cell::new(0),
                    points: UnsafeCell::new(Vec::with_capacity(points)),
                    capacity: points,
                })
            })
            .collect();
        ComWrapper::new(ParameterChanges {
            queues,
            used: Cell::new(0),
            dropped: Cell::new(0),
        })
    }

    pub fn clear(&self) {
        for q in &self.queues[..self.used.get()] {
            q.points().clear();
        }
        self.used.set(0);
    }

    fn queue_for(&self, id: ParamID) -> Option<usize> {
        let used = self.used.get();
        if let Some(i) = self.queues[..used].iter().position(|q| q.id.get() == id) {
            return Some(i);
        }
        if used == self.queues.len() {
            return None;
        }
        self.queues[used].id.set(id);
        self.used.set(used + 1);
        Some(used)
    }

    /// Adds a point for `id`; drops and counts it when the pool is full.
    pub fn add(&self, id: ParamID, offset: i32, value: ParamValue) -> bool {
        let ok = self
            .queue_for(id)
            .and_then(|i| self.queues[i].add(offset, value))
            .is_some();
        if !ok {
            self.dropped.set(self.dropped.get() + 1);
        }
        ok
    }

    pub fn len(&self) -> usize {
        self.used.get()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The parameter and its last value in queue `index`.
    pub fn last(&self, index: usize) -> Option<(ParamID, ParamValue)> {
        let q = self.queues[..self.used.get()].get(index)?;
        Some((q.id.get(), q.last_value()?))
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.get()
    }

    fn queue_ptr(&self, index: usize) -> *mut IParamValueQueue {
        self.queues[index]
            .as_com_ref::<IParamValueQueue>()
            .map(|r| r.as_ptr())
            .unwrap_or(std::ptr::null_mut())
    }
}

impl IParameterChangesTrait for ParameterChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.used.get() as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if index < 0 || index as usize >= self.used.get() {
            return std::ptr::null_mut();
        }
        self.queue_ptr(index as usize)
    }

    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut int32,
    ) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: checked for null above.
        let Some(i) = self.queue_for(unsafe { *id }) else {
            self.dropped.set(self.dropped.get() + 1);
            return std::ptr::null_mut();
        };
        if !index.is_null() {
            // SAFETY: optional out-pointer, checked for null.
            unsafe { *index = i as int32 };
        }
        self.queue_ptr(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vst3::ComRef;

    #[test]
    fn event_list_is_bounded() {
        let l = EventList::with_capacity(2);
        let e: Event = unsafe { std::mem::zeroed() };
        assert!(l.push(e) && l.push(e));
        assert!(!l.push(e));
        assert_eq!(unsafe { l.getEventCount() }, 2);
        assert_eq!(l.dropped(), 1);
        let mut out: Event = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { l.getEvent(5, &mut out) }, kInvalidArgument);
        l.clear();
        assert!(l.is_empty());
    }

    #[test]
    fn parameter_changes_group_points_by_id() {
        let c = ParameterChanges::with_capacity(2, 4);
        assert!(c.add(10, 0, 0.1));
        assert!(c.add(20, 0, 0.2));
        assert!(c.add(10, 0, 0.3)); // same offset: replaces
        assert!(c.add(10, 5, 0.4));
        assert!(c.add(10, 3, 0.7)); // MIDI and automation can be supplied separately
        assert!(!c.add(30, 0, 1.0)); // pool full
        assert_eq!(c.len(), 2);
        assert_eq!(c.dropped(), 1);
        let q = unsafe { ComRef::from_raw(c.getParameterData(0)) }.unwrap();
        assert_eq!(unsafe { q.getParameterId() }, 10);
        assert_eq!(unsafe { q.getPointCount() }, 3);
        let (mut o, mut v) = (0, 0.0);
        unsafe { q.getPoint(0, &mut o, &mut v) };
        assert_eq!((o, v), (0, 0.3));
        unsafe { q.getPoint(1, &mut o, &mut v) };
        assert_eq!((o, v), (3, 0.7));
        assert_eq!(c.last(1), Some((20, 0.2)));
        // The plugin adds output changes the same way.
        c.clear();
        let mut index = -1;
        let q = unsafe { c.addParameterData(&7, &mut index) };
        assert_eq!(index, 0);
        let q = unsafe { ComRef::from_raw(q) }.unwrap();
        unsafe { q.addPoint(3, 0.9, std::ptr::null_mut()) };
        assert_eq!(c.last(0), Some((7, 0.9)));
    }
}
