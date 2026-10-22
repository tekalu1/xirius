//! A VST3 plugin's state as a `.vstpreset` file (the form DAWproject keeps it in), and the
//! text a song keeps it as (`XV3S`, see `xirius-plugins`' `state.rs`).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const HEADER: usize = 48;
const MAGIC: &[u8; 4] = b"XV3S";

/// The component and controller streams of a plugin's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub component: Vec<u8>,
    pub controller: Vec<u8>,
}

/// The song's text of a state.
pub fn to_text(s: &State) -> String {
    let mut b = Vec::with_capacity(12 + s.component.len() + s.controller.len());
    b.extend_from_slice(MAGIC);
    for part in [&s.component, &s.controller] {
        b.extend_from_slice(&(part.len() as u32).to_le_bytes());
        b.extend_from_slice(part);
    }
    STANDARD.encode(b)
}

/// The state in a song's text; `None` when it is not the `XV3S` form.
pub fn from_text(text: &str) -> Option<State> {
    let b = STANDARD.decode(text.trim()).ok()?;
    let rest = b.strip_prefix(MAGIC.as_slice())?;
    let (component, rest) = take(rest)?;
    let (controller, rest) = take(rest)?;
    rest.is_empty().then_some(State {
        component,
        controller,
    })
}

fn take(b: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let len = u32::from_le_bytes(b.get(..4)?.try_into().ok()?) as usize;
    let body = b.get(4..4usize.checked_add(len)?)?;
    Some((body.to_vec(), &b[4 + len..]))
}

/// A `.vstpreset` of the class `class_id` (32 hex digits).
pub fn encode(class_id: &str, s: &State) -> Vec<u8> {
    let mut id = [b'0'; 32];
    for (d, c) in id.iter_mut().zip(class_id.bytes().filter(u8::is_ascii)) {
        *d = c;
    }
    let mut chunks: Vec<(&[u8; 4], u64, u64)> = Vec::new();
    let mut b = Vec::with_capacity(HEADER + s.component.len() + s.controller.len() + 64);
    b.extend_from_slice(b"VST3");
    b.extend_from_slice(&1i32.to_le_bytes());
    b.extend_from_slice(&id);
    b.extend_from_slice(&0i64.to_le_bytes());
    for (tag, data) in [(b"Comp", &s.component), (b"Cont", &s.controller)] {
        if tag == b"Cont" && data.is_empty() {
            continue;
        }
        chunks.push((tag, b.len() as u64, data.len() as u64));
        b.extend_from_slice(data);
    }
    let list = b.len() as i64;
    b[40..48].copy_from_slice(&list.to_le_bytes());
    b.extend_from_slice(b"List");
    b.extend_from_slice(&(chunks.len() as i32).to_le_bytes());
    for (tag, at, len) in chunks {
        b.extend_from_slice(tag);
        b.extend_from_slice(&(at as i64).to_le_bytes());
        b.extend_from_slice(&(len as i64).to_le_bytes());
    }
    b
}

/// The class id and state of a `.vstpreset`; `None` when it is not one.
pub fn decode(b: &[u8]) -> Option<(String, State)> {
    if b.get(..4)? != b"VST3" {
        return None;
    }
    let id = std::str::from_utf8(b.get(8..40)?).ok()?.to_string();
    let list = usize::try_from(i64::from_le_bytes(b.get(40..48)?.try_into().ok()?)).ok()?;
    if b.get(list..list.checked_add(4)?)? != b"List" {
        return None;
    }
    let n = i32::from_le_bytes(b.get(list + 4..list + 8)?.try_into().ok()?);
    let mut s = State::default();
    for i in 0..usize::try_from(n).ok()? {
        let e = list + 8 + i * 20;
        let tag = b.get(e..e + 4)?;
        let at =
            usize::try_from(i64::from_le_bytes(b.get(e + 4..e + 12)?.try_into().ok()?)).ok()?;
        let len =
            usize::try_from(i64::from_le_bytes(b.get(e + 12..e + 20)?.try_into().ok()?)).ok()?;
        let data = b.get(at..at.checked_add(len)?)?.to_vec();
        match tag {
            b"Comp" => s.component = data,
            b"Cont" => s.controller = data,
            _ => {}
        }
    }
    Some((id, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_survives_the_preset_and_the_text() {
        let s = State {
            component: vec![1, 2, 3, 0, 255],
            controller: vec![9, 8],
        };
        let id = "ABCDEF0123456789ABCDEF0123456789";
        let (back_id, back) = decode(&encode(id, &s)).unwrap();
        assert_eq!((back_id.as_str(), &back), (id, &s));
        assert_eq!(from_text(&to_text(&s)), Some(s.clone()));
        // A state without a controller stream keeps it empty.
        let one = State {
            component: vec![7],
            controller: vec![],
        };
        assert_eq!(decode(&encode(id, &one)).unwrap().1, one);
        assert!(decode(b"VST3 but too short").is_none());
        assert!(from_text("AAEC").is_none());
    }
}
