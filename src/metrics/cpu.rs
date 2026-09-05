//! CPU utilisation derived from two snapshots of the kernel's tick counters.

use std::time::Duration;

use crate::sys::{self, CpuTimes, SysError};

/// Busy share of a [`CpuTimes`] delta, as a percentage.
///
/// A zero-length delta (both snapshots identical) reports 0.0 rather than NaN.
#[must_use]
pub fn percentage_from_delta(delta: &CpuTimes) -> f32 {
    let total = delta.total();
    if total == 0 {
        0.0
    } else {
        delta.busy() as f32 / total as f32 * 100.0
    }
}

/// Keeps the previous [`CpuTimes`] snapshot so repeated samples can be taken
/// without blocking, which is what the daemon mode needs.
#[derive(Clone, Debug, Default)]
pub struct CpuSampler {
    previous: Option<CpuTimes>,
}

impl CpuSampler {
    /// A sampler with no baseline yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { previous: None }
    }

    /// Take a snapshot and report the utilisation since the previous one.
    ///
    /// Returns `Ok(None)` on the very first call, where there is no baseline to
    /// diff against.
    ///
    /// # Errors
    ///
    /// Returns a [`SysError`] when the CPU counters cannot be read.
    pub fn sample(&mut self) -> Result<Option<f32>, SysError> {
        let current = sys::cpu_times()?;
        let percent = self
            .previous
            .as_ref()
            .map(|previous| percentage_from_delta(&current.delta(previous)));
        self.previous = Some(current);
        Ok(percent)
    }
}

/// Measure CPU utilisation over `delay`, blocking for that long.
///
/// This is what one-line mode uses: two snapshots separated by a sleep.
///
/// # Errors
///
/// Returns a [`SysError`] when the CPU counters cannot be read.
pub fn cpu_percentage(delay: Duration) -> Result<f32, SysError> {
    let mut sampler = CpuSampler::new();
    sampler.sample()?;
    std::thread::sleep(delay);
    Ok(sampler.sample()?.unwrap_or(0.0))
}

/// The sampling delay for a refresh interval in seconds.
///
/// Mirrors the original's `interval * 1_000_000 - 10_000` microseconds: sample
/// for slightly less than the interval so the status line is ready in time.
#[must_use]
pub fn sampling_delay(interval_secs: u64) -> Duration {
    Duration::from_millis(interval_secs.saturating_mul(1000).saturating_sub(10))
}

#[cfg(test)]
mod tests {
    use super::{percentage_from_delta, sampling_delay};
    use crate::sys::CpuTimes;
    use std::time::Duration;

    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 1e-4
    }

    #[test]
    fn quarter_busy_delta_is_twenty_five_percent() {
        let delta = CpuTimes {
            user: 20,
            nice: 3,
            system: 2,
            idle: 75,
        };
        assert!(close(percentage_from_delta(&delta), 25.0));
    }

    #[test]
    fn zero_delta_is_zero_percent() {
        assert!(close(percentage_from_delta(&CpuTimes::default()), 0.0));
    }

    #[test]
    fn fully_busy_delta_is_one_hundred_percent() {
        let delta = CpuTimes {
            user: 100,
            nice: 0,
            system: 0,
            idle: 0,
        };
        assert!(close(percentage_from_delta(&delta), 100.0));
    }

    #[test]
    fn sampling_delay_trims_ten_milliseconds() {
        assert_eq!(sampling_delay(1), Duration::from_millis(990));
        assert_eq!(sampling_delay(2), Duration::from_millis(1990));
    }
}
