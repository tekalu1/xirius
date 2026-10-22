//! What the plugin can call on the host. Requests only raise flags; the instance's main
//! thread and the callers act on them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use clack_extensions::audio_ports::{AudioPortRescanFlags, HostAudioPorts, HostAudioPortsImpl};
use clack_extensions::latency::{HostLatency, HostLatencyImpl};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::note_name::{HostNoteName, HostNoteNameImpl};
use clack_extensions::note_ports::{
    HostNotePorts, HostNotePortsImpl, NoteDialects, NotePortRescanFlags,
};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags,
};
use clack_extensions::state::{HostState, HostStateImpl};
use clack_host::prelude::*;

/// Requests from the plugin, read by [`Plugin`](crate::Plugin) and the main thread.
#[derive(Default)]
pub(crate) struct Flags {
    pub(crate) callback: AtomicBool,
    pub(crate) restart: AtomicBool,
    pub(crate) flush: AtomicBool,
    pub(crate) dirty: AtomicBool,
    pub(crate) latency_changed: AtomicBool,
    pub(crate) params_changed: AtomicBool,
    pub(crate) ports_changed: AtomicBool,
    /// The plugin said its note names changed (`clap.note-name`).
    pub(crate) note_names_changed: AtomicBool,
    /// Whether the plugin is activated, so edits go through `process` rather than `flush`.
    pub(crate) active: AtomicBool,
    /// The latency read at the last activation, or after the plugin said it changed.
    pub(crate) latency: AtomicU32,
    /// Whether an [`AudioPart`](crate::AudioPart) is alive.
    pub(crate) audio_alive: AtomicBool,
}

impl Flags {
    pub(crate) fn take(flag: &AtomicBool) -> bool {
        flag.swap(false, Ordering::AcqRel)
    }
}

pub(crate) struct XiriusHost;

impl HostHandlers for XiriusHost {
    type Shared<'a> = HostShared;
    type MainThread<'a> = HostMain;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostState>()
            .register::<HostParams>()
            .register::<HostLatency>()
            .register::<HostAudioPorts>()
            .register::<HostNotePorts>()
            .register::<HostNoteName>();
    }
}

pub(crate) struct HostShared {
    pub(crate) flags: Arc<Flags>,
}

impl SharedHandler<'_> for HostShared {
    fn request_restart(&self) {
        self.flags.restart.store(true, Ordering::Release);
    }

    fn request_process(&self) {}

    fn request_callback(&self) {
        self.flags.callback.store(true, Ordering::Release);
    }
}

impl HostLogImpl for HostShared {
    fn log(&self, severity: LogSeverity, message: &str) {
        // Only what tells of a fault is kept; plugins can be chatty.
        if matches!(
            severity,
            LogSeverity::Error
                | LogSeverity::Fatal
                | LogSeverity::HostMisbehaving
                | LogSeverity::PluginMisbehaving
        ) {
            eprintln!("clap: {severity:?}: {message}");
        }
    }
}

impl HostParamsImplShared for HostShared {
    fn request_flush(&self) {
        self.flags.flush.store(true, Ordering::Release);
    }
}

pub(crate) struct HostMain {
    pub(crate) flags: Arc<Flags>,
}

impl MainThreadHandler<'_> for HostMain {}

impl HostStateImpl for HostMain {
    fn mark_dirty(&self) {
        self.flags.dirty.store(true, Ordering::Release);
    }
}

impl HostParamsImplMainThread for HostMain {
    fn rescan(&self, _flags: ParamRescanFlags) {
        self.flags.params_changed.store(true, Ordering::Release);
    }

    fn clear(&self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

impl HostLatencyImpl for HostMain {
    fn changed(&self) {
        self.flags.latency_changed.store(true, Ordering::Release);
    }
}

impl HostAudioPortsImpl for HostMain {
    fn is_rescan_flag_supported(&self, _flag: AudioPortRescanFlags) -> bool {
        // The ports are read again at each activation.
        true
    }

    fn rescan(&self, _flags: AudioPortRescanFlags) {
        self.flags.ports_changed.store(true, Ordering::Release);
    }
}

impl HostNotePortsImpl for HostMain {
    fn supported_dialects(&self) -> NoteDialects {
        NoteDialects::CLAP | NoteDialects::MIDI
    }

    fn rescan(&self, _flags: NotePortRescanFlags) {
        self.flags.ports_changed.store(true, Ordering::Release);
    }
}

impl HostNoteNameImpl for HostMain {
    fn changed(&self) {
        self.flags.note_names_changed.store(true, Ordering::Release);
    }
}
