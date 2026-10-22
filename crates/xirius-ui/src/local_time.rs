//! The wall clock of this computer, for the times on the screen (PRJ-011: when a step was made).

/// A moment in local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// The local time of `ms` since 1970 (UTC), or `None` when the system cannot say.
#[cfg(windows)]
pub fn local(ms: u64) -> Option<Local> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    // A FILETIME counts 100 ns from 1601.
    let ticks = (ms + 11_644_473_600_000).checked_mul(10_000)?;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut here: SYSTEMTIME = unsafe { std::mem::zeroed() };
    // SAFETY: both take pointers to locals that live through the call.
    unsafe {
        if FileTimeToSystemTime(&file, &mut utc) == 0 {
            return None;
        }
        if SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut here) == 0 {
            return None;
        }
    }
    Some(Local {
        year: here.wYear as i32,
        month: here.wMonth as u32,
        day: here.wDay as u32,
        hour: here.wHour as u32,
        minute: here.wMinute as u32,
    })
}

/// The local time of `ms` since 1970 (UTC), or `None` when the system cannot say.
#[cfg(unix)]
pub fn local(ms: u64) -> Option<Local> {
    let t = (ms / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r writes only to `tm`.
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return None;
    }
    Some(Local {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
    })
}

#[cfg(not(any(windows, unix)))]
pub fn local(_ms: u64) -> Option<Local> {
    None
}

/// When `at_ms` was, as a clock shows it: `14:05` on the day of `now_ms`, `10/14 14:05` before.
pub fn clock(now_ms: u64, at_ms: u64) -> String {
    let (Some(at), Some(now)) = (local(at_ms), local(now_ms)) else {
        return String::new();
    };
    if (at.year, at.month, at.day) == (now.year, now.month, now.day) {
        format!("{}:{:02}", at.hour, at.minute)
    } else {
        format!("{}/{} {}:{:02}", at.month, at.day, at.hour, at.minute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_on_the_same_day_shows_the_hour_and_minute_and_another_day_its_date_too() {
        let now = 1_791_000_000_000; // 2026-10-02 (UTC)
        let here = local(now).expect("the system tells the local time");
        let c = clock(now, now);
        assert_eq!(c, format!("{}:{:02}", here.hour, here.minute));
        let before = now - 3 * 86_400_000;
        let then = local(before).unwrap();
        assert_eq!(
            clock(now, before),
            format!(
                "{}/{} {}:{:02}",
                then.month, then.day, then.hour, then.minute
            )
        );
        // A minute later is a minute later on the clock, whatever the zone.
        let next = local(now + 60_000).unwrap();
        assert_eq!(
            (next.hour * 60 + next.minute) % 1440,
            (here.hour * 60 + here.minute + 1) % 1440
        );
    }
}
