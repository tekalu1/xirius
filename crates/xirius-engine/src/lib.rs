//! Xirius audio engine: a mixer graph run on a real-time thread, device output and plugin hosting.

pub mod anticipation;
pub mod automation_time;
pub mod builtin;
pub mod clap_plugin;
pub mod click;
pub mod control;
pub mod crash;
pub mod device;
pub mod devices;
pub mod disk_stream;
pub mod engine;
pub mod exclusive;
pub mod midi_in;
pub mod midi_out;
pub mod mixer;
pub mod monitor;
mod parallel;
pub mod plugin;
pub mod prefetch;
pub mod processor;
pub mod record;
pub mod rt;
pub mod rt_lock;
pub mod sandbox;
pub mod server;
pub mod stats;
pub mod stock;
pub mod storage;
pub mod tap;
pub mod timeline;
pub mod timing;
pub mod tone;
pub mod wavfile;
pub mod wire;

pub use control::ControlState;
pub use engine::{AudioEngine, Command, Controller, EngineConfig, EngineEvent, render_offline};
pub use mixer::{BusSpec, MidiTarget, MixSpec, Output, Plan, SendSpec, TrackSlot, TrackSpec};
pub use processor::{
    AudioIo, CTRL_AFTERTOUCH, CTRL_PITCH_BEND, NoteEvent, NoteKind, ProcessSetup, Processor,
    Readied, RenderMode,
};
pub use timeline::{AudioData, EnvPoint, Region, Shape, TimedNote, Timeline};
