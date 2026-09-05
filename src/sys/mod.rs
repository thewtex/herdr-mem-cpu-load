//! Platform abstraction over the raw system counters.
//!
//! Every backend exposes the same four sampling entry points so the metric and
//! render layers stay platform independent: Linux reads `/proc`, macOS asks
//! Mach, Windows asks the Win32 kernel counters, and everything else gets a
//! stub that fails cleanly.

use std::fmt;

pub mod filetime;

#[cfg(unix)]
// `unix_common` is compiled for every Unix target, but only the Linux and
// macOS backends re-export it; on a Unix target that falls back to
// `unsupported` nothing calls it.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
mod unix_common;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as backend;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as backend;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use unsupported as backend;

/// Cumulative CPU jiffies since boot, in the four categories the original
/// `tmux-mem-cpu-load` formula uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
}

impl CpuTimes {
    /// Ticks spent doing work: user + nice + system.
    #[must_use]
    pub const fn busy(&self) -> u64 {
        self.user + self.nice + self.system
    }

    /// Ticks accounted for in total: busy + idle.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.busy() + self.idle
    }

    /// Field-wise difference against an earlier snapshot.
    ///
    /// Counters can be reset (for example when a CPU is hot-unplugged), so each
    /// field saturates at zero rather than wrapping.
    #[must_use]
    pub const fn delta(&self, earlier: &Self) -> Self {
        Self {
            user: self.user.saturating_sub(earlier.user),
            nice: self.nice.saturating_sub(earlier.nice),
            system: self.system.saturating_sub(earlier.system),
            idle: self.idle.saturating_sub(earlier.idle),
        }
    }
}

/// Physical memory usage, stored in bytes so no precision is lost before the
/// render layer decides on a unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryStatus {
    pub used_bytes: u64,
    pub total_bytes: u64,
}

impl MemoryStatus {
    /// Used memory in megabytes.
    #[must_use]
    pub fn used_mb(&self) -> f32 {
        self.used_bytes as f32 / (1024.0 * 1024.0)
    }

    /// Total memory in megabytes.
    #[must_use]
    pub fn total_mb(&self) -> f32 {
        self.total_bytes as f32 / (1024.0 * 1024.0)
    }

    /// Memory that is not counted as used.
    #[must_use]
    pub const fn free_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used_bytes)
    }

    /// Used memory as a percentage of the total, or 0 when the total is 0.
    #[must_use]
    pub fn used_percent(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.used_bytes as f32 / self.total_bytes as f32 * 100.0
        }
    }
}

/// The one, five, and fifteen minute load averages.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadAverages {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// Error returned when a system counter cannot be read or understood.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SysError(String);

impl SysError {
    /// Build an error from anything that can become a `String`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SysError {}

impl From<std::io::Error> for SysError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

/// Read the cumulative CPU counters.
///
/// # Errors
///
/// Returns a [`SysError`] when the platform counters cannot be read or parsed.
pub fn cpu_times() -> Result<CpuTimes, SysError> {
    backend::cpu_times()
}

/// Read the current physical memory usage.
///
/// # Errors
///
/// Returns a [`SysError`] when the platform counters cannot be read or parsed.
pub fn memory_status() -> Result<MemoryStatus, SysError> {
    backend::memory_status()
}

/// Read the one, five, and fifteen minute load averages.
///
/// # Errors
///
/// Returns a [`SysError`] when the platform does not report load averages.
pub fn load_averages() -> Result<LoadAverages, SysError> {
    backend::load_averages()
}

/// Number of online logical CPUs. Never fails; falls back to 1.
#[must_use]
pub fn cpu_count() -> u32 {
    backend::cpu_count()
}

/// Shared fallback for backends whose native CPU count query failed.
#[must_use]
pub(crate) fn cpu_count_fallback() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

/// Publish emulated load averages for [`load_averages`] to report.
///
/// Windows has no kernel load average, so the daemon runs a
/// [`crate::metrics::load_emulator::LoadEmulator`] and hands the result here
/// before every `metrics::collect`.
#[cfg(windows)]
pub fn publish_emulated_load(averages: LoadAverages) {
    backend::publish_emulated_load(averages);
}

/// Seed the emulated load averages from a single sample, unless the daemon has
/// already published a decayed set.
///
/// Called by `metrics::collect` so one-line mode, which takes one measurement
/// and exits, still prints three numbers on Windows.
#[cfg(windows)]
pub fn seed_emulated_load(busy_cores: f64) {
    backend::seed_emulated_load(busy_cores);
}

#[cfg(test)]
mod tests {
    use super::{CpuTimes, MemoryStatus};

    #[test]
    fn a_delta_saturates_when_the_counters_go_backwards() {
        // Mach reports 32 bit tick counters that wrap, and a CPU can be taken
        // offline underneath a sampler. Neither may panic.
        let earlier = CpuTimes {
            user: 100,
            nice: 50,
            system: 70,
            idle: 900,
        };
        let later = CpuTimes {
            user: 10,
            nice: 0,
            system: 90,
            idle: 1000,
        };

        let delta = later.delta(&earlier);
        assert_eq!(delta.user, 0);
        assert_eq!(delta.nice, 0);
        assert_eq!(delta.system, 20);
        assert_eq!(delta.idle, 100);
        assert_eq!(delta.busy(), 20);
        assert_eq!(delta.total(), 120);
    }

    #[test]
    fn used_memory_above_the_total_does_not_underflow() {
        let memory = MemoryStatus {
            used_bytes: 8,
            total_bytes: 4,
        };
        assert_eq!(memory.free_bytes(), 0);
    }
}
