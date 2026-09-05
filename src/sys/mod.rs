//! Platform abstraction over the raw system counters.
//!
//! Every backend exposes the same four sampling entry points so the metric and
//! render layers stay platform independent. Linux is implemented today; macOS
//! and Windows arrive in a later phase and only need to provide the same four
//! free functions.

use std::fmt;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;

// Phase 03 adds `src/sys/macos.rs` and `src/sys/windows.rs`; uncommenting the
// two arms below and narrowing the `unsupported` cfg to match is all the
// wiring they need. The declarations stay commented out rather than
// `cfg`-gated because rustfmt resolves every `mod` regardless of its `cfg`
// and errors out when the file does not exist yet.
//
// #[cfg(target_os = "macos")]
// mod macos;
// #[cfg(target_os = "macos")]
// use macos as backend;
//
// #[cfg(windows)]
// mod windows;
// #[cfg(windows)]
// use windows as backend;

#[cfg(not(target_os = "linux"))]
mod unsupported;
#[cfg(not(target_os = "linux"))]
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
