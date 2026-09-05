//! Sampling that every Unix backend shares.
//!
//! `getloadavg` and `sysconf(_SC_NPROCESSORS_ONLN)` are identical on Linux and
//! macOS, so both backends re-export these instead of carrying a copy each.

use super::{cpu_count_fallback, LoadAverages, SysError};

/// The one, five, and fifteen minute load averages from `getloadavg`.
///
/// # Errors
///
/// Returns a [`SysError`] when the kernel reports fewer than three averages.
pub(crate) fn load_averages() -> Result<LoadAverages, SysError> {
    let mut averages = [0.0_f64; 3];
    // SAFETY: `getloadavg` writes at most `nelem` doubles into the buffer, and
    // the buffer has room for exactly three.
    let filled = unsafe { libc::getloadavg(averages.as_mut_ptr(), 3) };
    if filled < 3 {
        return Err(SysError::new("getloadavg did not report three averages"));
    }
    Ok(LoadAverages {
        one: averages[0],
        five: averages[1],
        fifteen: averages[2],
    })
}

/// Online logical CPUs, falling back to the shared chain when `sysconf` cannot
/// answer.
pub(crate) fn cpu_count() -> u32 {
    // SAFETY: `sysconf` is a pure query with no pointer arguments.
    let online = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if online > 0 {
        online as u32
    } else {
        cpu_count_fallback()
    }
}
