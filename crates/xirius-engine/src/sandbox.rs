//! The shared memory between the engine and a plugin in `xirius-plugin-host` (PLG-023): one
//! mapping per plugin instance with a [`Header`], an "in" and an "out" ring of interleaved
//! stereo frames and a ring of notes, plus two named events. The engine writes the input and
//! the notes and sets "req"; the host processes, writes the output and sets "done".
//!
//! Each ring has one writer and one reader, which publish their stream positions (in frames
//! since the mapping was made) in the header. The out ring runs [`Header::latency`] frames
//! ahead of the in ring, so the host has that long for every block (see [`crate::server`]).

use std::io;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, EVENT_MODIFY_STATE, OpenEventW, SYNCHRONIZATION_SYNCHRONIZE, SetEvent,
    WaitForSingleObject,
};

use crate::processor::{ProcessSetup, RenderMode};

pub const MAGIC: u32 = u32::from_le_bytes(*b"XSBX");
pub const VERSION: u32 = 5;
/// The rings start here; the header fits well below.
const RING_OFFSET: usize = 4096;
/// Notes the engine can be ahead of the host.
pub const NOTE_CAPACITY: usize = 1024;

#[repr(C)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    /// Frames per audio ring, a power of two.
    pub capacity: u32,
    pub max_block: u32,
    pub sample_rate: f64,
    pub offline: u32,
    /// The plugin makes sound from notes; its input is silence.
    pub instrument: u32,
    /// How far the out ring runs ahead of the in ring, in frames.
    pub latency: u32,
    /// The plugin's own latency, written by the host when the instance is ready.
    pub plugin_latency: AtomicU32,
    /// The host process generation whose instance serves this memory; written last when the
    /// instance is ready. 0 until then.
    pub ready_epoch: AtomicU64,
    pub in_write: AtomicU64,
    pub in_read: AtomicU64,
    pub out_write: AtomicU64,
    pub out_read: AtomicU64,
    pub note_write: AtomicU64,
    pub note_read: AtomicU64,
    /// Blocks the host has processed.
    pub host_blocks: AtomicU64,
    /// The time the plugin has spent processing them in the host, in ns (ENG-060). Added
    /// before `in_read` is published, so a reader that sees the frames sees their time.
    pub host_ns: AtomicU64,
    /// Notes the engine dropped because the note ring was full.
    pub notes_dropped: AtomicU64,
    /// The host process generation that crashed inside this instance's plugin, written by the
    /// host as it goes down; 0 when none has. Tells the engine which plugin took the host down.
    pub faulted: AtomicU64,
}

/// The `on` of a [`NoteRecord`] that is a controller change: `pitch` is the controller number
/// and `velocity` its value (EDT-004).
pub const NOTE_CONTROL: u8 = 2;
/// A normalized plugin parameter change, carrying `param_id` and `param_value`.
pub const NOTE_PARAM: u8 = 3;

/// A note at a stream position of the in ring.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NoteRecord {
    pub frame: u64,
    pub pitch: u8,
    pub on: u8,
    pub pad: [u8; 2],
    pub velocity: f32,
    pub param_id: u32,
    pub param_value: f64,
}

#[derive(Clone, Copy)]
pub enum Ring {
    In = 0,
    Out = 1,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The shared memory of one instance.
pub struct SharedRings {
    mapping: HANDLE,
    view: *mut u8,
    mask: usize,
}

// The view is plain memory; who may touch which part is the ring protocol's business, and the
// positions are atomics.
unsafe impl Send for SharedRings {}
unsafe impl Sync for SharedRings {}

impl SharedRings {
    fn size(capacity: usize) -> usize {
        RING_OFFSET + 2 * capacity * 2 * size_of::<f32>() + NOTE_CAPACITY * size_of::<NoteRecord>()
    }

    /// Creates the memory (zeroed) and writes the header.
    pub fn create(
        name: &str,
        capacity: usize,
        setup: &ProcessSetup,
        latency: u32,
        instrument: bool,
    ) -> io::Result<SharedRings> {
        assert!(capacity.is_power_of_two());
        let size = Self::size(capacity);
        // SAFETY: a pagefile-backed mapping; the name is NUL-terminated.
        let mapping = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                (size as u64 >> 32) as u32,
                size as u32,
                wide(name).as_ptr(),
            )
        };
        let rings = Self::map(mapping, capacity)?;
        // SAFETY: the view is at least RING_OFFSET bytes, page-aligned, and nobody else has it.
        unsafe {
            (rings.view as *mut Header).write(Header {
                magic: MAGIC,
                version: VERSION,
                capacity: capacity as u32,
                max_block: setup.max_block as u32,
                sample_rate: setup.sample_rate,
                offline: (setup.mode == RenderMode::Offline) as u32,
                instrument: instrument as u32,
                latency,
                plugin_latency: AtomicU32::new(0),
                ready_epoch: AtomicU64::new(0),
                in_write: AtomicU64::new(0),
                in_read: AtomicU64::new(0),
                out_write: AtomicU64::new(0),
                out_read: AtomicU64::new(0),
                note_write: AtomicU64::new(0),
                note_read: AtomicU64::new(0),
                host_blocks: AtomicU64::new(0),
                host_ns: AtomicU64::new(0),
                notes_dropped: AtomicU64::new(0),
                faulted: AtomicU64::new(0),
            });
        }
        Ok(rings)
    }

    /// Opens memory made by [`SharedRings::create`] and checks its header.
    pub fn open(name: &str) -> io::Result<SharedRings> {
        // SAFETY: the name is NUL-terminated.
        let mapping = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide(name).as_ptr()) };
        let mut rings = Self::map(mapping, 0)?;
        let h = rings.header();
        if h.magic != MAGIC || h.version != VERSION || !(h.capacity as usize).is_power_of_two() {
            return Err(io::Error::other("not a xirius sandbox mapping"));
        }
        rings.mask = h.capacity as usize - 1;
        Ok(rings)
    }

    fn map(mapping: HANDLE, capacity: usize) -> io::Result<SharedRings> {
        if mapping.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a valid mapping handle; map all of it.
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view.Value.is_null() {
            let e = io::Error::last_os_error();
            // SAFETY: we own the handle.
            unsafe { CloseHandle(mapping) };
            return Err(e);
        }
        Ok(SharedRings {
            mapping,
            view: view.Value as *mut u8,
            mask: capacity.saturating_sub(1),
        })
    }

    pub fn header(&self) -> &Header {
        // SAFETY: written by `create` before anyone else could open the mapping.
        unsafe { &*(self.view as *const Header) }
    }

    pub fn capacity(&self) -> usize {
        self.mask + 1
    }

    fn ring(&self, ring: Ring) -> *mut f32 {
        // SAFETY: both rings lie inside the view (see `size`).
        unsafe {
            self.view
                .add(RING_OFFSET + ring as usize * self.capacity() * 2 * size_of::<f32>())
                as *mut f32
        }
    }

    fn notes(&self) -> *mut NoteRecord {
        // SAFETY: the note ring follows the two audio rings (see `size`).
        unsafe {
            self.view
                .add(RING_OFFSET + 2 * self.capacity() * 2 * size_of::<f32>())
                as *mut NoteRecord
        }
    }

    /// Writes `l`/`r` at stream position `pos`.
    ///
    /// # Safety
    /// The caller is the ring's only writer and the frames are free (not yet to be read).
    pub unsafe fn write(&self, ring: Ring, pos: u64, l: &[f32], r: &[f32]) {
        let base = self.ring(ring);
        for (i, (&a, &b)) in l.iter().zip(r).enumerate() {
            let k = ((pos as usize).wrapping_add(i) & self.mask) * 2;
            // SAFETY: k + 1 < 2 * capacity.
            unsafe {
                base.add(k).write(a);
                base.add(k + 1).write(b);
            }
        }
    }

    /// Writes `frames` frames of silence at stream position `pos`.
    ///
    /// # Safety
    /// As for [`SharedRings::write`].
    pub unsafe fn write_silence(&self, ring: Ring, pos: u64, frames: usize) {
        let base = self.ring(ring);
        for i in 0..frames {
            let k = ((pos as usize).wrapping_add(i) & self.mask) * 2;
            // SAFETY: k + 1 < 2 * capacity.
            unsafe {
                base.add(k).write(0.0);
                base.add(k + 1).write(0.0);
            }
        }
    }

    /// Reads into `l`/`r` from stream position `pos`.
    ///
    /// # Safety
    /// The caller is the ring's only reader and the frames have been published.
    pub unsafe fn read(&self, ring: Ring, pos: u64, l: &mut [f32], r: &mut [f32]) {
        let base = self.ring(ring);
        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let k = ((pos as usize).wrapping_add(i) & self.mask) * 2;
            // SAFETY: k + 1 < 2 * capacity.
            unsafe {
                *a = base.add(k).read();
                *b = base.add(k + 1).read();
            }
        }
    }

    /// Adds a note to the note ring; false (and counted) when the ring is full. For the engine,
    /// the ring's only writer.
    pub fn push_note(&self, note: NoteRecord) -> bool {
        let h = self.header();
        let w = h.note_write.load(Ordering::Relaxed);
        if w - h.note_read.load(Ordering::Acquire) >= NOTE_CAPACITY as u64 {
            h.notes_dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        // SAFETY: the slot is free and we are the only writer.
        unsafe {
            self.notes()
                .add(w as usize % NOTE_CAPACITY)
                .write_volatile(note)
        };
        h.note_write.store(w + 1, Ordering::Release);
        true
    }

    /// Free event slots, for the engine's single writer before publishing a block.
    pub fn note_slots(&self) -> usize {
        let h = self.header();
        NOTE_CAPACITY
            - (h.note_write.load(Ordering::Relaxed) - h.note_read.load(Ordering::Acquire)) as usize
    }

    /// The oldest note not yet taken, if it starts before `before`. For the host, the ring's
    /// only reader.
    pub fn take_note_before(&self, before: u64) -> Option<NoteRecord> {
        let h = self.header();
        let r = h.note_read.load(Ordering::Relaxed);
        if r == h.note_write.load(Ordering::Acquire) {
            return None;
        }
        // SAFETY: the engine published this slot.
        let note = unsafe { self.notes().add(r as usize % NOTE_CAPACITY).read_volatile() };
        if note.frame >= before {
            return None;
        }
        h.note_read.store(r + 1, Ordering::Release);
        Some(note)
    }

    /// Drops every note not yet taken. For the host, when a fresh instance takes over.
    pub fn skip_notes(&self) {
        let h = self.header();
        h.note_read
            .store(h.note_write.load(Ordering::Acquire), Ordering::Release);
    }
}

impl Drop for SharedRings {
    fn drop(&mut self) {
        // SAFETY: we own the view and the handle.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view as *mut _,
            });
            CloseHandle(self.mapping);
        }
    }
}

/// A named auto-reset event.
pub struct Event(HANDLE);

unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    pub fn create(name: &str) -> io::Result<Event> {
        let name = wide(name);
        // SAFETY: the name is NUL-terminated.
        let h = unsafe {
            CreateEventW(
                std::ptr::null(),
                0,
                0,
                if name.len() == 1 {
                    std::ptr::null()
                } else {
                    name.as_ptr()
                },
            )
        };
        Self::checked(h)
    }

    pub fn open(name: &str) -> io::Result<Event> {
        // SAFETY: the name is NUL-terminated.
        let h = unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                0,
                wide(name).as_ptr(),
            )
        };
        Self::checked(h)
    }

    fn checked(h: HANDLE) -> io::Result<Event> {
        if h.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Event(h))
        }
    }

    pub fn handle(&self) -> HANDLE {
        self.0
    }

    /// Does not block; safe to call on the audio thread.
    pub fn set(&self) {
        // SAFETY: a valid event handle.
        unsafe { SetEvent(self.0) };
    }

    /// True when signalled within `ms`.
    pub fn wait(&self, ms: u32) -> bool {
        // SAFETY: a valid event handle.
        unsafe { WaitForSingleObject(self.0, ms) == WAIT_OBJECT_0 }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_wait_for_their_frame_and_the_ring_fills_up() {
        let setup = ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 64,
            mode: RenderMode::Offline,
        };
        let name = format!("Local\\xirius-sandbox-test-{}", std::process::id());
        let rings = SharedRings::create(&name, 1024, &setup, 64, true).unwrap();
        let other = SharedRings::open(&name).unwrap();
        assert_eq!(other.header().instrument, 1);
        let note = |frame| NoteRecord {
            frame,
            pitch: 60,
            on: 1,
            velocity: 0.5,
            ..NoteRecord::default()
        };
        assert!(rings.push_note(note(10)));
        assert!(rings.push_note(note(70)));
        assert_eq!(other.take_note_before(64), Some(note(10)));
        assert_eq!(other.take_note_before(64), None);
        assert_eq!(other.take_note_before(128), Some(note(70)));
        let param = NoteRecord {
            frame: 145,
            on: NOTE_PARAM,
            param_id: u32::MAX,
            param_value: 0.123456789012345,
            ..NoteRecord::default()
        };
        assert!(rings.push_note(param));
        assert_eq!(other.take_note_before(145), None);
        assert_eq!(other.take_note_before(146), Some(param));
        for i in 0..NOTE_CAPACITY as u64 {
            assert!(rings.push_note(note(i)));
        }
        assert!(!rings.push_note(note(0)));
        assert_eq!(rings.header().notes_dropped.load(Ordering::Relaxed), 1);
        other.skip_notes();
        assert_eq!(other.take_note_before(u64::MAX), None);
    }
}
