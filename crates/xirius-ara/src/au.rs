//! ARA through Audio Units (`ARAAudioUnit.h`, PLG-033) is macOS only, and Xirius does not
//! host Audio Units yet. On macOS the binding would ask the AU for the property
//! `kAudioUnitProperty_ARAFactory` and, per instance, `kAudioUnitProperty_ARAPlugInExtensionBindingWithRoles`.
//! This file marks the place; nothing is built.

/// Whether this build can bind ARA through Audio Units.
pub const SUPPORTED: bool = false;
