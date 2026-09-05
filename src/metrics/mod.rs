//! Platform independent metric layer built on top of [`crate::sys`].

pub mod cpu;
pub mod load;
pub mod memory;

use crate::sys::{self, SysError};
pub use crate::sys::{LoadAverages, MemoryStatus};

/// CPU percentage output mode.
///
/// * `Default` caps at 100% across all processors, e.g. `51.2%`.
/// * `Threads` scales by the thread count, e.g. `410%` on 8 threads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CpuMode {
    #[default]
    Default = 0,
    Threads = 1,
}

impl TryFrom<u8> for CpuMode {
    type Error = SysError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Default),
            1 => Ok(Self::Threads),
            other => Err(SysError::new(format!(
                "invalid cpu mode `{other}`, expected 0 or 1"
            ))),
        }
    }
}

/// One complete reading of the system, ready to be rendered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub cpu_percent: f32,
    pub memory: MemoryStatus,
    pub load: LoadAverages,
    pub cpu_count: u32,
}

/// Collect memory, load, and CPU count around an already measured CPU
/// percentage.
///
/// The CPU percentage is passed in because measuring it requires two snapshots
/// separated by a delay, which the caller controls.
///
/// # Errors
///
/// Returns a [`SysError`] when the memory or load counters cannot be read.
pub fn collect(cpu_percent: f32) -> Result<Sample, SysError> {
    Ok(Sample {
        cpu_percent,
        memory: sys::memory_status()?,
        load: sys::load_averages()?,
        cpu_count: sys::cpu_count(),
    })
}

#[cfg(test)]
mod tests {
    use super::CpuMode;

    #[test]
    fn cpu_mode_round_trips_from_u8() {
        assert_eq!(CpuMode::try_from(0), Ok(CpuMode::Default));
        assert_eq!(CpuMode::try_from(1), Ok(CpuMode::Threads));
        assert!(CpuMode::try_from(2).is_err());
        assert_eq!(CpuMode::default(), CpuMode::Default);
    }
}
