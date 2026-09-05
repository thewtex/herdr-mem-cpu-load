//! `FILETIME` arithmetic for the Windows backend.
//!
//! This lives outside `windows.rs` so it compiles — and is therefore unit
//! tested — on every platform, not only on the one machine that can run it.

use super::CpuTimes;

/// Join the two halves of a `FILETIME` into the 64-bit count of 100-nanosecond
/// intervals it really is.
#[must_use]
pub fn filetime_to_u64(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

/// Map the three counters `GetSystemTimes` reports onto the shared
/// [`CpuTimes`].
///
/// Windows counts idle time *inside* the kernel total and has no separate
/// "nice" category, so kernel time needs idle subtracted before the shared
/// busy/total formula gives the right percentage.
#[must_use]
pub fn cpu_times_from_system_times(idle: u64, kernel: u64, user: u64) -> CpuTimes {
    CpuTimes {
        user,
        nice: 0,
        system: kernel.saturating_sub(idle),
        idle,
    }
}

#[cfg(test)]
mod tests {
    use super::{cpu_times_from_system_times, filetime_to_u64};
    use crate::metrics::cpu::percentage_from_delta;

    #[test]
    fn a_filetime_is_its_two_halves_joined() {
        assert_eq!(filetime_to_u64(1, 5), (1 << 32) + 5);
        assert_eq!(filetime_to_u64(0, 0), 0);
        assert_eq!(filetime_to_u64(0, u32::MAX), u64::from(u32::MAX));
        assert_eq!(filetime_to_u64(u32::MAX, u32::MAX), u64::MAX);
    }

    #[test]
    fn kernel_time_has_idle_subtracted_out() {
        let times = cpu_times_from_system_times(100, 300, 200);
        assert_eq!(times.user, 200);
        assert_eq!(times.nice, 0);
        assert_eq!(times.system, 200);
        assert_eq!(times.idle, 100);
        assert_eq!(times.busy(), 400);
        assert_eq!(times.total(), 500);
        assert!((percentage_from_delta(&times) - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_idle_time_above_the_kernel_total_saturates() {
        // The three counters are sampled one after another, so a pathological
        // reading must not underflow.
        let times = cpu_times_from_system_times(300, 100, 0);
        assert_eq!(times.system, 0);
    }
}
