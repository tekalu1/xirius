//! A plugin's saved state as the text a song keeps (`plugin.state.set --state`).
//!
//! The VST3 component and controller streams are kept apart: `XV3S`, then each stream as a
//! little-endian `u32` length and its bytes, all in base64.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use xirius_vst3::PluginState;

const MAGIC: &[u8; 4] = b"XV3S";

pub fn encode_state(state: &PluginState) -> String {
    let mut b = Vec::with_capacity(12 + state.component.len() + state.controller.len());
    b.extend_from_slice(MAGIC);
    for part in [&state.component, &state.controller] {
        b.extend_from_slice(&(part.len() as u32).to_le_bytes());
        b.extend_from_slice(part);
    }
    STANDARD.encode(b)
}

pub fn decode_state(text: &str) -> Result<PluginState, String> {
    let b = STANDARD
        .decode(text.trim())
        .map_err(|e| format!("状態が base64 形式ではありません: {e}"))?;
    let rest = b
        .strip_prefix(MAGIC.as_slice())
        .ok_or("状態が VST3 の形式（XV3S）ではありません")?;
    let (component, rest) = take(rest)?;
    let (controller, rest) = take(rest)?;
    if !rest.is_empty() {
        return Err("状態の末尾に余分なバイトがあります".into());
    }
    Ok(PluginState {
        component,
        controller,
    })
}

fn take(b: &[u8]) -> Result<(Vec<u8>, &[u8]), String> {
    let short = || "状態のデータが途中で切れています".to_string();
    let len: [u8; 4] = b.get(..4).ok_or_else(short)?.try_into().unwrap();
    let len = u32::from_le_bytes(len) as usize;
    let body = b.get(4..4 + len).ok_or_else(short)?;
    Ok((body.to_vec(), &b[4 + len..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_survives_the_text() {
        let s = PluginState {
            component: vec![1, 2, 3, 0, 255],
            controller: vec![],
        };
        assert_eq!(decode_state(&encode_state(&s)).unwrap(), s);
        assert!(decode_state("AAEC").is_err());
        let cut = &encode_state(&s)[..8];
        assert!(decode_state(cut).is_err());
    }
}
