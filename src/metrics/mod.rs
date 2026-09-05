//! Platform independent metric layer built on top of [`crate::sys`].

pub mod cpu;
pub mod load;
pub mod load_emulator;
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
    let cpu_count = sys::cpu_count();

    // Windows has no kernel load average. The daemon keeps a
    // [`load_emulator::LoadEmulator`] up to date and this seed is a no-op for
    // it; one-line mode has no such history, so its single reading stands in
    // for all three windows.
    #[cfg(windows)]
    sys::seed_emulated_load(busy_cores(cpu_percent, cpu_count));

    Ok(Sample {
        cpu_percent,
        memory: sys::memory_status()?,
        load: sys::load_averages()?,
        cpu_count,
    })
}

/// A CPU percentage expressed as the number of fully busy cores it represents,
/// which is the unit a load average is in.
#[must_use]
pub fn busy_cores(cpu_percent: f32, cpu_count: u32) -> f64 {
    f64::from(cpu_percent) / 100.0 * f64::from(cpu_count)
}

#[cfg(test)]
mod tests {
    use super::{busy_cores, CpuMode};

    #[test]
    fn cpu_mode_round_trips_from_u8() {
        assert_eq!(CpuMode::try_from(0), Ok(CpuMode::Default));
        assert_eq!(CpuMode::try_from(1), Ok(CpuMode::Threads));
        assert!(CpuMode::try_from(2).is_err());
        assert_eq!(CpuMode::default(), CpuMode::Default);
    }

    #[test]
    fn a_cpu_percentage_converts_to_busy_cores() {
        assert!((busy_cores(50.0, 8) - 4.0).abs() < f64::EPSILON);
        assert!((busy_cores(100.0, 8) - 8.0).abs() < f64::EPSILON);
        assert!(busy_cores(0.0, 8).abs() < f64::EPSILON);
        assert!(busy_cores(50.0, 0).abs() < f64::EPSILON);
    }
}
