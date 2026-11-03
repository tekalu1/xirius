//! Note names, with C3 = 60 (UI-335): the roll, the keyboards, the Sampler's and the Drum
//! Kit's ranges and the commands all name a pitch the same way, so 0 is C-2 and 127 is G8.

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// "C3", "F#2": the name of a MIDI pitch.
pub fn name(pitch: u8) -> String {
    format!("{}{}", NAMES[usize::from(pitch % 12)], octave(pitch))
}

/// The octave a pitch is named in (60 is in octave 3).
pub fn octave(pitch: u8) -> i32 {
    i32::from(pitch) / 12 - 2
}

/// The pitch a name like "C3", "f#2", "Bb-1" or "c-2" means, or a plain number "60".
/// `None` for anything else or a pitch outside 0..=127.
pub fn parse(text: &str) -> Option<u8> {
    let t = text.trim();
    if let Ok(n) = t.parse::<i64>() {
        return u8::try_from(n).ok().filter(|p| *p <= 127);
    }
    let mut chars = t.chars();
    let base: i32 = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (semi, rest) = if let Some(r) = rest.strip_prefix(['#', '♯']) {
        (base + 1, r)
    } else if let Some(r) = rest.strip_prefix(['b', '♭']) {
        (base - 1, r)
    } else {
        (base, rest)
    };
    let octave: i32 = rest.parse().ok()?;
    let pitch = (octave + 2) * 12 + semi;
    u8::try_from(pitch).ok().filter(|p| *p <= 127)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_335_sixty_is_c3_zero_is_c_minus_2_and_127_is_g8() {
        assert_eq!(name(60), "C3");
        assert_eq!(name(0), "C-2");
        assert_eq!(name(127), "G8");
        assert_eq!(name(61), "C#3");
        assert_eq!(name(69), "A3");
    }

    #[test]
    fn names_read_back_to_their_pitch_and_numbers_stay_numbers() {
        for p in 0..=127u8 {
            assert_eq!(parse(&name(p)), Some(p), "{}", name(p));
        }
        assert_eq!(parse("c3"), Some(60));
        assert_eq!(parse("Bb2"), Some(58));
        assert_eq!(parse(" 60 "), Some(60));
        assert_eq!(parse("128"), None);
        assert_eq!(parse("G#8"), None, "above 127");
        assert_eq!(parse("Cb-2"), None, "below 0");
        assert_eq!(parse("H3"), None);
        assert_eq!(parse("C"), None);
    }
}
