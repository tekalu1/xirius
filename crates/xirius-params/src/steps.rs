//! Which parameters of a VST3 or CLAP plug-in are switches or lists of choices (AUT-016):
//! their lanes hold one value up to the next point, as a built-in's switch does. What a
//! plug-in tells of a parameter's steps is trusted only when it reads as a switch or as a
//! list; anything else (a whole number, a count no list has) glides, and the plug-in takes
//! the value it is given as it always did.

/// The most choices a plug-in's list is taken to have; past it the count is not trusted and
/// the parameter glides.
pub const MAX_LIST: u32 = 128;

/// How many values a VST3 parameter takes when it is a switch or a list: a step count of 1
/// (two values), or the list flag with a step count a list can have. A larger whole number
/// without the flag, a continuous parameter (0), a count that is not one, or one past
/// [`MAX_LIST`] glides.
pub fn vst3_steps(step_count: i32, is_list: bool) -> Option<u32> {
    match step_count {
        1 => Some(2),
        n if is_list && n > 1 && n < MAX_LIST as i32 => Some(n as u32 + 1),
        _ => None,
    }
}

/// [`vst3_steps`] for a CLAP parameter: stepped with two values, or an enum of a list's size
/// (its range a whole number of steps).
pub fn clap_steps(stepped: bool, is_enum: bool, min: f64, max: f64) -> Option<u32> {
    if !stepped || !min.is_finite() || !max.is_finite() || max <= min {
        return None;
    }
    let count = (max - min).round();
    if (max - min - count).abs() > 1e-9 {
        return None;
    }
    match count as i64 {
        1 => Some(2),
        n if is_enum && n > 1 && n < i64::from(MAX_LIST) => Some(n as u32 + 1),
        _ => None,
    }
}
