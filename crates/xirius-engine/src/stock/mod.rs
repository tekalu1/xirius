//! The built-in instruments and effects (PLG-016): processors whose parameters follow the
//! common spec of [`xirius_params`] (PLG-045).
//!
//! [`create`] makes the processor of a built-in and a [`StockHandle`] that keeps its
//! parameters. The song holds each value as 0..1; the handle takes them, the processor reads
//! them on the audio thread when one changed, so automation and edits reach a running mix
//! without a new plan. Nothing here allocates once prepared.

pub mod bank;
mod compressor;
mod delay;
mod drum_kit;
pub mod dsp;
mod eq;
mod limiter;
mod reverb;
mod sampler;
mod samples;
#[cfg(test)]
#[allow(dead_code)]
mod testing;

use std::sync::Arc;

pub use bank::StockHandle;
use bank::{ParamBank, Params};
use xirius_params::BuiltinDef;

use crate::processor::Processor;

/// The processor and handle of the built-in `key` names (its uid or name), or `None` when no
/// built-in has that name or it has no processor yet.
pub fn create(key: &str) -> Option<(Box<dyn Processor>, StockHandle)> {
    let def = xirius_params::find(key)?;
    let bank = Arc::new(ParamBank::new(def));
    let processor = make(def, Params::new(bank.clone()))?;
    Some((processor, StockHandle::new(bank)))
}

fn make(def: &'static BuiltinDef, params: Params) -> Option<Box<dyn Processor>> {
    Some(match def.uid {
        "eq" => Box::new(eq::Eq::new(params)),
        "compressor" => Box::new(compressor::Compressor::new(params)),
        "limiter" => Box::new(limiter::Limiter::new(params)),
        "delay" => Box::new(delay::Delay::new(params)),
        "reverb" => Box::new(reverb::Reverb::new(params)),
        "sampler" => Box::new(sampler::Sampler::new(params)),
        "drum-kit" => Box::new(drum_kit::DrumKit::new(params)),
        _ => return None,
    })
}
